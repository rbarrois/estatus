use std::env;
use std::path;

use estatus;

fn main() {
    let paths: Vec<path::PathBuf> = env::args().skip(1).map(|arg| path::PathBuf::from(arg)).collect();
    let results = estatus::statuses(paths, &path::PathBuf::from("/var/db/pkg"));
    let results = results.unwrap();
    for result in results.values() {
        println!("{:?}: {}", result.status, result.path.display());
    }
}
