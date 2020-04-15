use super::*;

const EXAMPLE_PATH: &'static str = "/example";

#[test]
fn parse_hash_md5() {
    // echo -n 'Hello, world!' | md5sum
    let raw_hash = "6cd3556deb0da54bca060b4c39479839";
    assert_ok_eq(
        FileHash::MD5([108, 211, 85, 109, 235, 13, 165, 75, 202, 6, 11, 76, 57, 71, 152, 57]),
        parse_hash(EXAMPLE_PATH.as_ref(), 42, &raw_hash),
    );
}

#[test]
fn parse_hash_md5_mixed_case() {
    // echo -n 'Hello, world!' | md5sum
    let raw_hash = "6CD3556DEB0DA54BCa060b4c39479839";
    assert_ok_eq(
        FileHash::MD5([108, 211, 85, 109, 235, 13, 165, 75, 202, 6, 11, 76, 57, 71, 152, 57]),
        parse_hash(EXAMPLE_PATH.as_ref(), 42, &raw_hash),
    );
}

#[test]
fn parse_hash_md5_bad() {
    // Hash with invalid chars for hex
    let raw_hash = "6cd3556deZ0da54!ca060=4c39479839";
    let result = parse_hash(EXAMPLE_PATH.as_ref(), 42, &raw_hash);
    assert!(result.is_err());
}

#[test]
fn parse_hash_unhandled() {
    let raw_hash = "sha1:943a702d06f34599aee1f8da8ef9f7296031d699";
    let result = parse_hash(EXAMPLE_PATH.as_ref(), 42, &raw_hash);
    assert!(result.is_err());
}

#[test]
fn read_entry_obj() {
    let raw_line = "obj /usr/bin/rustc-1.41.1 1bcc8fefbc19ba3faf51564bf2a0e180 1586621688";
    assert_ok_eq(
        VarDBEntry {
            path: path::PathBuf::from("/usr/bin/rustc-1.41.1"),
            metadata: FileMetadata::Regular {
                mtime: 1586621688,
                hash: FileHash::MD5([27, 204, 143, 239, 188, 25, 186, 63, 175, 81, 86, 75, 242, 160, 225, 128]),
            },
        },
        read_entry(EXAMPLE_PATH.as_ref(), 42, &raw_line),
    );
}

#[test]
fn read_entry_sym() {
    let raw_line = "sym /usr/bin/rustc -> rustc-1.41.1 1586621688";
    assert_ok_eq(
        VarDBEntry {
            path: path::PathBuf::from("/usr/bin/rustc"),
            metadata: FileMetadata::Symlink {
                mtime: 1586621688,
                dest: path::PathBuf::from("rustc-1.41.1"),
            },
        },
        read_entry(EXAMPLE_PATH.as_ref(), 42, &raw_line),
    );
}

#[test]
fn vardbentry_in_tree() {
    let entry = VarDBEntry {
        path: path::PathBuf::from("/usr/lib/rustlib"),
        metadata: FileMetadata::Directory,
    };
    assert!(entry.in_tree(&vec![path::PathBuf::from("/etc"), path::PathBuf::from("/usr/lib")]));
}

#[test]
fn vardbentry_not_in_tree() {
    let entry = VarDBEntry {
        path: path::PathBuf::from("/usr/lib64/rustlib"),
        metadata: FileMetadata::Directory,
    };
    assert!(! entry.in_tree(&vec![path::PathBuf::from("/etc"), path::PathBuf::from("/usr/lib")]));
}

fn assert_ok_eq<T: PartialEq + fmt::Debug>(expected: T, value: Result<T, Error>) -> () {
    assert!(value.is_ok());
    assert_eq!(expected, value.unwrap());
}
