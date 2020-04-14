use std::collections;
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::io;
use std::path;
use std::time;
use std::fmt;
use std::error;

use md5::{Md5, Digest};

mod vardbapi;

#[derive(Eq, PartialEq, Debug)]
pub enum FileType {
    FIFO,
    CHR,  // Character device
    DIR,
    BLK,  // Block device
    REG,  // Regular file
    LNK,  // Symbolic link
    SOCK,
}

impl From<fs::FileType> for FileType {
    fn from(item: fs::FileType) -> Self {
        if cfg!(unix) && item.is_block_device() {
            Self::BLK
        } else if cfg!(unix) && item.is_char_device() {
            Self::CHR
        } else if cfg!(unix) && item.is_fifo() {
            Self::FIFO
        } else if cfg!(unix) && item.is_socket() {
            Self::SOCK
        } else if item.is_dir() {
            Self::DIR
        } else if item.is_symlink() {
            Self::LNK
        } else if item.is_file() {
            Self::REG
        } else {
            panic!("Invalid file type {:?}", item);
        }
    }
}

#[derive(Eq, PartialEq, Debug)]
pub enum FileStatus {
    Aligned,  // Complies with the expected hash
    Touched,  // Right type and content, wrong mtime
    Altered,  // Right type, wrong content
    Changed,  // Wrong type
    Absent,   // Expected file is not present on disk
    Orphan,   // File on disk, not owned by any package
}

#[derive(Eq, PartialEq, Debug)]
pub struct ResultItem {
    pub path: path::PathBuf,
    pub ftype: FileType,
    pub status: FileStatus,
}


type MD5Hash = [u8; 16];
type SearchPaths = Vec<path::PathBuf>;
pub type ResultSet = collections::HashMap<path::PathBuf, ResultItem>;


fn compute_md5(path: &path::Path) -> io::Result<MD5Hash> {
    let mut hasher = Md5::new();
    let mut file = fs::File::open(path)?;
    io::copy(&mut file, &mut hasher)?;
    let digest = hasher.result().into();
    Ok(digest)
}

fn changed_hash(path: &path::Path, hash: &vardbapi::FileHash) -> Result<bool, Error> {
    match hash {
        vardbapi::FileHash::MD5(expected_md5) => {
            let actual = compute_md5(&path).map_err(|e| Error::FileUnreadable { path: path.into(), source: e })?;
            Ok(expected_md5 != &actual)
        }
    }
}



fn check_file(entry: &fs::DirEntry, expected: Option<&vardbapi::FileMetadata>) -> Result<ResultItem, Error> {
    let metadata = entry.metadata().map_err(|e| Error::FileUnreadable { path: entry.path(), source: e })?;
    let ftype = FileType::from(metadata.file_type());
    let entry_mtime = metadata
        .modified().map_err(|e| Error::FileUnreadable { path: entry.path(), source: e })?
        .duration_since(time::UNIX_EPOCH).expect("Bad mtime").as_secs();

    if let Some(vardbapi::FileMetadata::Regular { mtime, hash, .. }) = expected {
        if ftype != FileType::REG {
            Ok(ResultItem {
                path: entry.path(),
                ftype: ftype,
                status: FileStatus::Changed,
            })
        } else if &entry_mtime != mtime {
            if changed_hash(&entry.path(), &hash)? {
                Ok(ResultItem {
                    path: entry.path(),
                    ftype: ftype,
                    status: FileStatus::Altered,
                })
            } else {
                Ok(ResultItem {
                    path: entry.path(),
                    ftype: ftype,
                    status: FileStatus::Touched,
                })
            }
        } else {
            Ok(ResultItem {
                path: entry.path(),
                ftype: ftype,
                status: FileStatus::Aligned,
            })
        }
    } else {
        Ok(ResultItem {
            path: entry.path(),
            ftype: FileType::from(metadata.file_type()),
            status: FileStatus::Orphan,
        })
    }
}


fn check_dir(base: &path::Path, store: &vardbapi::VarDB, mut output: &mut ResultSet) -> Result<(), Error> {
    let entries = fs::read_dir(base).map_err(|e| Error::DirUnreadable { path: base.into(), source: e })?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::DirUnreadable { path: base.into(), source: e })?;
        let metadata = entry.metadata().map_err(|e| Error::DirUnreadable { path: entry.path(), source: e })?;
        if metadata.is_dir() {
            check_dir(&entry.path(), store, &mut output)?;
        } else {
            let result = check_file(&entry, store.get(&entry.path()))?;
            output.insert(entry.path(), result);
        }
    }
    Ok(())
}


pub fn statuses(paths: impl IntoIterator<Item=path::PathBuf>, vdb_root: &path::Path) -> Result<ResultSet, Error> {
    let paths_list: Vec<path::PathBuf> = paths.into_iter().collect();

    let expectations = vardbapi::get_vdb(vdb_root, &paths_list)
        .map_err(|e| Error::VarDBError { source: e })?;
    let mut results = ResultSet::new();

    for base in paths_list.iter() {
        check_dir(base, &expectations, &mut results)?;
    }
    Ok(results)
}

#[derive(Debug)]
pub enum Error {
    VarDBError {
        source: vardbapi::Error,
    },
    DirUnreadable {
        path: path::PathBuf,
        source: io::Error,
    },
    FileUnreadable {
        path: path::PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::VarDBError { ref source } => {
                write!(f, "{}", source)
            },
            Error::DirUnreadable { ref path, ref source } => {
                write!(f, "{}: could not read directory: {}", path.display(), source)
            },
            Error::FileUnreadable { ref path, ref source } => {
                write!(f, "{}: could not read file: {}", path.display(), source)
            },
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            Error::VarDBError { ref source, ..} => Some(source),
            Error::DirUnreadable { ref source, ..} => Some(source),
            Error::FileUnreadable { ref source, ..} => Some(source),
        }
    }
}
