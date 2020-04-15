use std::collections;
use std::io::BufRead;
use std::num;
use std::fmt;
use std::error;
use std::io;
use std::path;
use std::fs;

#[cfg(test)]
mod tests;

/**
 * Public structs and type aliases
 */

/// A possible expected file hash.
///
/// Based on supported values in vardbapi; for now, only MD5 is planned.
#[derive(Eq, PartialEq, Debug)]
pub enum FileHash {
    MD5(super::MD5Hash),
}

/// Type alias for the mtime recorded in vardbapi.
///
/// For now, only accurate to the second.
type LowResSystemTime = u64;

/// Metadata from a vardbapi entry.
///
#[derive(Eq, PartialEq, Debug)]
pub enum FileMetadata {
    Regular {
        mtime: LowResSystemTime,
        hash: FileHash,
    },
    Directory,
    Device,
    Fifo,
    Symlink {
        mtime: LowResSystemTime,
        dest: path::PathBuf,
    },
}


#[derive(Eq, PartialEq, Debug)]
pub struct VarDBEntry {
    pub path: path::PathBuf,
    pub metadata: FileMetadata,
}


impl VarDBEntry {
    fn in_tree(&self, bases: &super::SearchPaths) -> bool {
        bases.iter().all(|base| self.path.starts_with(base))
    }
}


/// Abstraction over the vardbapi.
pub type VarDB = collections::HashMap<path::PathBuf, FileMetadata>;


/// Parse the VarDB.
///
/// This function returns an iterator containing both successfully parsed entries,
/// and errors; this allows the caller to decide whether processing should stop
/// at the first error or continue.
pub fn parse_vdb<'a>(vdb_root: &'a path::Path) -> Result<impl Iterator<Item=Result<VarDBEntry, Error>> + 'a, Error> {
    let categories = fs::read_dir(vdb_root);
    categories
        .map_err(|e| Error::VarDBUnreadable { path: vdb_root.into(), source: e})
        .map(
            |entries| entries
            .flat_map(move |category| {
                if let Ok(entry) = category {
                    read_category(entry)
                } else {
                    Err(Error::VarDBUnreadable { path: vdb_root.clone().into(), source: category.unwrap_err()})
                }.inline_err()
            }))
}

/// Fetch the VarDB, as a single HashMap.
///
/// Processing will halt at the first error.
pub fn get_vdb(vdb_root: &path::Path, bases: &super::SearchPaths) -> Result<VarDB, Error> {
    let mut vdb = VarDB::new();
    let filtered = parse_vdb(vdb_root)?.filter(
        |entry| match entry {
            Err(_) => true,
            Ok(dbentry) => dbentry.in_tree(&bases),
        });
    for entry in filtered {
        let entry = entry?;
        vdb.insert(entry.path, entry.metadata);
    }
    Ok(vdb)
}

fn read_category(direntry: fs::DirEntry) -> Result<impl Iterator<Item=Result<VarDBEntry, Error>>, Error> {
    let atoms = fs::read_dir(direntry.path());
    atoms
        .map_err(|e| Error::CategoryUnreadable { path: direntry.path(), source: e })
        .map(
            |entries| entries
            .flat_map(move |atom| {
                if let Ok(entry) = atom {
                    read_atom(entry)
                } else {
                    Err(Error::CategoryUnreadable { path: direntry.path(), source: atom.unwrap_err() })
                }.inline_err()
            }))
}

fn read_atom(direntry: fs::DirEntry) -> Result<impl Iterator<Item=Result<VarDBEntry, Error>>, Error> {
    let fname = direntry.file_name().into_string().map_err(
        |_| Error::AtomInvalidName { path: direntry.path() } )?;
    if fname.starts_with("-MERGING-") {
        return Err(Error::AtomInvalidName { path: direntry.path() });
    }
    let contents = direntry.path().join("CONTENTS");
    let f = fs::File::open(&contents);
    if let Err(e) = f {
        return Err(Error::ContentsUnreadable { path: contents.clone(), source: e });
    }
    Ok(io::BufReader::new(f.unwrap())
        .lines()
        .enumerate()
        .map(move |(i, l)| {
            if let Ok(entry) = l {
                read_entry(&contents, i + 1, &entry)
            } else {
                Err(Error::EntryCorrupted { path: contents.clone(), line: i + 1, source: l.unwrap_err() })
            }
        }))
}

fn read_entry(contents: &path::Path, line: usize, entry: &str) -> Result<VarDBEntry, Error> {
    if entry.len() < 5 {
        return Err(Error::EntryMissingField { path: contents.into(), line: line, raw: entry.into() });
    }
    let prefix = &entry[0..3];
    match prefix {
        "obj" => parse_obj(&contents, line, &entry),
        "dir" => Ok(VarDBEntry {
            path: entry[4..].into(),
            metadata: FileMetadata::Directory,
        }),
        "dev" => Ok(VarDBEntry {
            path: entry[4..].into(),
            metadata: FileMetadata::Device,
        }),
        "fif" => Ok(VarDBEntry {
            path: entry[4..].into(),
            metadata: FileMetadata::Fifo,
        }),
        "sym" => parse_sym(&contents, line, &entry),
        _ => Err(Error::EntryUnhandledType { path: contents.into(), line: line, raw: entry.into() }),
    }
}

fn parse_obj(contents: &path::Path, line: usize, entry: &str) -> Result<VarDBEntry, Error> {
    let mut parts = entry[4..].rsplitn(3, ' ');
    // mtime: POSIX timestamp
    let mtime = parts.next().ok_or(Error::EntryMissingField { path: contents.into(), line, raw: entry.into() })?;
    let mtime = mtime.parse().map_err(|e| Error::EntryInvalidMTime { path: contents.into(), line, raw: mtime.into(), source: e })?;

    // Hash
    let raw_hash = parts.next().ok_or(Error::EntryMissingField { path: contents.into(), line, raw: entry.into() })?;
    let hash = parse_hash(&contents, line, &raw_hash)?;

    let path = parts.next().ok_or(Error::EntryMissingField { path: contents.into(), line, raw: entry.into() })?;

    Ok(VarDBEntry {
        path: path::PathBuf::from(path),
        metadata: FileMetadata::Regular {
            mtime: mtime,
            hash: hash,
        },
    })
}

fn parse_hash(contents: &path::Path, line: usize, raw_hash: &str) -> Result<FileHash, Error> {
    if raw_hash.len() == 32 {
        let md5_hash = parse_md5(&raw_hash)
            .map_err(|e| Error::EntryInvalidHash { path: contents.into(), line, raw: raw_hash.into(), source: e })?;
        Ok(FileHash::MD5(md5_hash))
    } else {
        Err(Error::EntryUnhandledHash { path: contents.into(), line, raw: raw_hash.into() })
    }
}

fn parse_sym(contents: &path::Path, line: usize, entry: &str) -> Result<VarDBEntry, Error> {
    const SEPARATOR : &'static str = " -> ";
    let details = &entry[4..];
    let sep_index = details.find(SEPARATOR).ok_or(
        Error::EntryMissingField { path: contents.into(), line, raw: entry.into() })?;

    let path = &details[..sep_index];

    let mtime_index = details.rfind(' ').ok_or(
        Error::EntryMissingField { path: contents.into(), line, raw: entry.into() })?;
    let mtime = &details[1 + mtime_index..];
    let mtime = mtime.parse().map_err(|e| Error::EntryInvalidMTime { path: contents.into(), line, raw: mtime.into(), source: e })?;

    let dest = &details[sep_index + SEPARATOR.len()..mtime_index];
    Ok(VarDBEntry {
        path: path::PathBuf::from(path),
        metadata: FileMetadata::Symlink {
            mtime: mtime,
            dest: path::PathBuf::from(dest),
        },
    })
}



fn parse_md5(text: &str) -> Result<super::MD5Hash, num::ParseIntError> {
    Ok([
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
    ])
}


/**
 * Errors
 */

#[derive(Debug)]
pub enum Error {
    /// Unable to open the varDB root
    VarDBUnreadable {
        path: path::PathBuf,
        source: io::Error,
    },
    /// Unable to read a category folder
    CategoryUnreadable {
        path: path::PathBuf,
        source: io::Error,
    },
    /// Unable to read an atom folder
    AtomUnreadable {
        path: path::PathBuf,
        source: io::Error,
    },
    /// 'Atom' folder with an invalid name,
    /// e.g. -MERGING-foo for an unfinished merge
    AtomInvalidName {
        path: path::PathBuf,
    },
    /// A `CONTENTS` file couldn't be read
    ContentsUnreadable {
        path: path::PathBuf,
        source: io::Error,
    },
    /// Reading a line failed
    /// e.g. invalid UTF-8
    EntryCorrupted {
        path: path::PathBuf,
        line: usize,
        source: io::Error
    },
    /// Unhandled entry type.
    ///
    /// An entry type is the line prefix: obj, dir, sym, ...
    EntryUnhandledType {
        path: path::PathBuf,
        line: usize,
        raw: String,
    },
    /// Missing a field for an entry.
    ///
    /// Since the list of fields differ by entry type,
    /// we'll stick to a generic error.
    ///
    /// XXX: Consider describing the missing field(s), or
    /// at least the expected syntax.
    EntryMissingField {
        path: path::PathBuf,
        line: usize,
        raw: String,
    },
    /// Failed to parse the `mtime` field.
    EntryInvalidMTime {
        path: path::PathBuf,
        line: usize,
        raw: String,
        source: num::ParseIntError,
    },
    /// Hash type isn't handled by this version of the code
    EntryUnhandledHash {
        path: path::PathBuf,
        line: usize,
        raw: String,
    },
    /// Failed to parse the `hash` field of an `obj` entry
    EntryInvalidHash {
        path: path::PathBuf,
        line: usize,
        raw: String,
        source: num::ParseIntError,
    },
    /// Failed to parse the `dest` field of a `sym` entry
    EntryInvalidDest {
        path: path::PathBuf,
        line: usize,
        raw: String,
        source: num::ParseIntError,
    },
}


impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::VarDBUnreadable {ref path, ref source} => {
                write!(f, "{}: could not open VarDB: {}", path.display(), source)
            },
            Error::CategoryUnreadable {ref path, ref source} => {
                write!(f, "{}: could not open category: {}", path.display(), source)
            },
            Error::AtomUnreadable {ref path, ref source} => {
                write!(f, "{}: could not open folder: {}", path.display(), source)
            },
            Error::AtomInvalidName {ref path} => {
                write!(f, "{}: potentially corrupted atom found", path.display())
            },
            Error::ContentsUnreadable {ref path, ref source} => {
                write!(f, "{}: could not open CONTENTS file: {}", path.display(), source)
            },
            Error::EntryCorrupted {ref path, line, ref source} => {
                write!(f, "{}:{}: entry corrupted: {}", path.display(), line, source)
            },
            Error::EntryUnhandledType {ref path, line, ref raw} => {
                write!(f, "{}:{}: unhandled entry type in \"{}\"", path.display(), line, raw)
            },
            Error::EntryMissingField {ref path, line, ref raw} => {
                write!(f, "{}:{}: missing field(s) in \"{}\"", path.display(), line, raw)
            },
            Error::EntryInvalidMTime {ref path, line, ref raw, ref source} => {
                write!(f, "{}:{}: could not parse mtime \"{}\": {}", path.display(), line, raw, source)
            },
            Error::EntryUnhandledHash {ref path, line, ref raw} => {
                write!(f, "{}:{}: unhandled hash found: {}", path.display(), line, raw)
            },
            Error::EntryInvalidHash {ref path, line, ref raw, ref source} => {
                write!(f, "{}:{}: could not parse hash \"{}\": {}", path.display(), line, raw, source)
            },
            Error::EntryInvalidDest {ref path, line, ref raw, ref source} => {
                write!(f, "{}:{}: could not parse symlink destination \"{}\": {}",
                       path.display(), line, raw, source)
            },
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            Error::VarDBUnreadable { ref source, .. } => Some(source),
            Error::CategoryUnreadable { ref source, .. } => Some(source),
            Error::AtomUnreadable { ref source, .. } => Some(source),
            Error::AtomInvalidName { .. } => None,
            Error::ContentsUnreadable { ref source, .. } => Some(source),
            Error::EntryCorrupted { ref source, .. } => Some(source),
            Error::EntryUnhandledType { .. } => None,
            Error::EntryMissingField { .. } => None,
            Error::EntryInvalidMTime { ref source, .. } => Some(source),
            Error::EntryUnhandledHash { .. } => None,
            Error::EntryInvalidHash { ref source, .. } => Some(source),
            Error::EntryInvalidDest { ref source, .. } => Some(source),
        }
    }
}


trait InlineErr {
    type Value;
    type Iter: Iterator<Item=Result<Self::Value, Self::Error>>;
    type Error;

    fn inline_err(self) -> InlinedErr<Self::Value, Self::Error, Self::Iter>;
}

impl<V, E, I: Iterator<Item=Result<V, E>>> InlineErr for Result<I, E> {
    type Value = V;
    type Iter = I;
    type Error = E;

    fn inline_err(self) -> InlinedErr<Self::Value, Self::Error, Self::Iter> {
        match self {
            Err(e) => InlinedErr::Error(Some(e)),
            Ok(i) => InlinedErr::Results(i),
        }
    }
}

enum InlinedErr<V, E, I: Iterator<Item=Result<V, E>>> {
    Error(Option<E>),
    Results(I),
}

impl<V, E, I: Iterator<Item=Result<V, E>>> Iterator for InlinedErr<V, E, I> {
    type Item = Result<V, E>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            InlinedErr::Error(err) => err.take().map(|e| Err(e)),
            InlinedErr::Results(iter) => iter.next(),
        }
    }
}
