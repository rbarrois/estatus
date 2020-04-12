use std::collections;
use std::error;
use std::fmt;
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::io::BufRead;
use std::io;
use std::num;
use std::path;
use std::time;

use md5::{Md5, Digest};


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

#[derive(Eq, PartialEq, Debug)]
enum FileHash {
    MD5(MD5Hash),
}

type LowResSystemTime = u64;

#[derive(Eq, PartialEq, Debug)]
struct FileMetadata {
    ftype: FileType,
    mtime: LowResSystemTime,
    hash: FileHash,
}


type Expectations = collections::HashMap<path::PathBuf, FileMetadata>;
type SearchPaths = Vec<path::PathBuf>;
pub type ResultSet = collections::HashMap<path::PathBuf, ResultItem>;


#[derive(Debug)]
enum DigestParseError {
    UnknownDigest,
    InvalidLength,
    Parse(num::ParseIntError),
}

impl fmt::Display for DigestParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            DigestParseError::UnknownDigest => write!(f, "Invalid digest type"),
            DigestParseError::InvalidLength => write!(f, "Digest is too short"),
            DigestParseError::Parse(ref e) => e.fmt(f),
        }
    }
}

impl error::Error for DigestParseError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            DigestParseError::UnknownDigest => None,
            DigestParseError::InvalidLength => None,
            DigestParseError::Parse(ref e) => Some(e),
        }
    }
}

impl From<num::ParseIntError> for DigestParseError {
    fn from(err: num::ParseIntError) -> DigestParseError {
        DigestParseError::Parse(err)
    }
}


fn compute_md5(path: &path::PathBuf) -> io::Result<MD5Hash> {
    let mut hasher = Md5::new();
    let mut file = fs::File::open(path)?;
    io::copy(&mut file, &mut hasher)?;
    let digest = hasher.result().into();
    Ok(digest)
}

fn changed_hash(path: &path::PathBuf, hash: &FileHash) -> io::Result<bool> {
    match hash {
        FileHash::MD5(expected_md5) => {
            let actual = compute_md5(&path)?;
            Ok(expected_md5 != &actual)
        }
    }
}


/// Return an iterator over folders in a given folder.
fn list_dirs(path: &path::PathBuf) -> io::Result<impl Iterator<Item=path::PathBuf>> {
    let entries = fs::read_dir(path)?;
    Ok(entries.filter_map(
            |result| result
            .ok()
            .and_then(|entry|
                      entry.metadata().ok()
                      .and_then(|meta| if meta.is_dir() { Some(entry.path()) } else { None })
                      )
            ))
}

fn parse_hash(text: &str) -> Result<FileHash, DigestParseError> {
    if text.len() == 32 {
        // MD5 hash
        Ok(FileHash::MD5([
            u8::from_str_radix(&text[0..2], 16)?,
            u8::from_str_radix(&text[2..4], 16)?,
            u8::from_str_radix(&text[4..6], 16)?,
            u8::from_str_radix(&text[6..8], 16)?,
            u8::from_str_radix(&text[8..10], 16)?,
            u8::from_str_radix(&text[10..12], 16)?,
            u8::from_str_radix(&text[12..14], 16)?,
            u8::from_str_radix(&text[14..16], 16)?,
            u8::from_str_radix(&text[16..18], 16)?,
            u8::from_str_radix(&text[18..20], 16)?,
            u8::from_str_radix(&text[20..22], 16)?,
            u8::from_str_radix(&text[22..24], 16)?,
            u8::from_str_radix(&text[24..26], 16)?,
            u8::from_str_radix(&text[26..28], 16)?,
            u8::from_str_radix(&text[28..30], 16)?,
            u8::from_str_radix(&text[30..32], 16)?,
        ]))
    } else {
        Err(DigestParseError::InvalidLength)
    }
}

fn parse_vdb(vdb_root: &path::PathBuf, bases: &SearchPaths) -> io::Result<Expectations> {
    let mut expectations = Expectations::new();
    for category in list_dirs(vdb_root)? {
        for atom in list_dirs(&category)? {
            let f = fs::File::open(atom.join("CONTENTS"))?;
            let f = io::BufReader::new(f);
            for line in f.lines() {
                let line = line?;
                // Line pattern:
                // dir /path/to/file
                // obj /path/to/file with empty.ext <md5-hash> <mtime>
                if line.starts_with("obj ") {
                    // <mtime>, <md5-hash>, obj /path/with wsp/file.ext
                    let mut parts = line[4..].rsplitn(3, ' ');
                    let mtime = parts.next().expect("Missing mtime!").parse().expect("Invalid mtime");
                    let hash = parts.next().expect("Missing hash!");
                    let path = path::PathBuf::from(parts.next().expect("Missing path!"));

                    if bases.iter().any(|base| path.starts_with(base)) {
                        expectations.entry(path)
                            .and_modify(|_prev| panic!("File dual-owned in {}", atom.display()))
                            .or_insert(FileMetadata {
                                hash: parse_hash(&hash).expect("Invalid hash!"),
                                mtime: mtime,
                                ftype: FileType::REG,
                            });
                    }
                }
            }
        }
    }
    Ok(expectations)
}


// XXX Note to self: pass in a callback that looks in the
// expectations, and performs an MD5 only if mtime don't match!


fn check_file(entry: &fs::DirEntry, expected: Option<&FileMetadata>) -> io::Result<ResultItem> {
    let metadata = entry.metadata()?;
    let ftype = FileType::from(metadata.file_type());
    if let Some(expected) = expected {
        if ftype != expected.ftype {
            Ok(ResultItem {
                path: entry.path(),
                ftype: ftype,
                status: FileStatus::Changed,
            })
        } else if metadata.modified()?.duration_since(time::UNIX_EPOCH).expect("Bad mtime").as_secs() != expected.mtime {
            if changed_hash(&entry.path(), &expected.hash)? {
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


fn check_dir(base: &path::PathBuf, store: &Expectations, mut output: &mut ResultSet) -> io::Result<()> {
    let entries = fs::read_dir(base)?;
    for entry in entries {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            check_dir(&entry.path(), store, &mut output)?;
        } else {
            let result = check_file(&entry, store.get(&entry.path()))?;
            output.insert(entry.path(), result);
        }
    }
    Ok(())
}


pub fn statuses(paths: impl IntoIterator<Item=path::PathBuf>, vdb_root: &path::PathBuf) -> io::Result<ResultSet> {
    let paths_list: Vec<path::PathBuf> = paths.into_iter().collect();

    let expectations = parse_vdb(vdb_root, &paths_list)?;
    let mut results = ResultSet::new();

    for base in paths_list.iter() {
        check_dir(base, &expectations, &mut results)?;
    }
    Ok(results)
}
