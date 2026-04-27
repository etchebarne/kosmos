use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::Path;

use crate::state::{Node, NodeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardOp {
    Cut,
    Copy,
}

pub fn read_dir(path: &Path) -> io::Result<Vec<Node>> {
    let mut nodes: Vec<Node> = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = if file_type.is_dir() {
            NodeKind::Directory
        } else {
            NodeKind::File
        };
        nodes.push(Node {
            path: entry.path(),
            name: name.into(),
            kind,
        });
    }
    nodes.sort_by(|a, b| match (a.kind, b.kind) {
        (NodeKind::Directory, NodeKind::File) => Ordering::Less,
        (NodeKind::File, NodeKind::Directory) => Ordering::Greater,
        _ => natural_cmp(a.name.as_ref(), b.name.as_ref()),
    });
    Ok(nodes)
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

pub fn create_file(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
}

pub fn create_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

pub fn delete(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

pub fn trash(path: &Path) -> io::Result<()> {
    trash::delete(path).map_err(|err| io::Error::other(err.to_string()))
}

pub fn move_into(src: &Path, dest_dir: &Path) -> io::Result<()> {
    let Some(name) = src.file_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source has no file name",
        ));
    };
    let dest = dest_dir.join(name);
    if dest == src {
        return Ok(());
    }
    if dest.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", dest.display()),
        ));
    }
    fs::rename(src, &dest)
}

pub fn paste(src: &Path, dest_dir: &Path, op: ClipboardOp) -> io::Result<()> {
    let Some(name) = src.file_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source has no file name",
        ));
    };
    let dest = unique_destination(dest_dir, name.to_string_lossy().as_ref());
    match op {
        ClipboardOp::Cut => {
            if dest == src {
                return Ok(());
            }
            fs::rename(src, &dest)
        }
        ClipboardOp::Copy => copy_recursive(src, &dest),
    }
}

fn unique_destination(dir: &Path, name: &str) -> std::path::PathBuf {
    let mut candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), Some(ext.to_string())),
        _ => (name.to_string(), None),
    };
    let mut counter = 1usize;
    loop {
        let new_name = match &ext {
            Some(ext) => format!("{stem} (copy {counter}).{ext}"),
            None => format!("{stem} (copy {counter})"),
        };
        candidate = dir.join(new_name);
        if !candidate.exists() {
            return candidate;
        }
        counter += 1;
    }
}

fn copy_recursive(src: &Path, dest: &Path) -> io::Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let child_dest = dest.join(entry.file_name());
            copy_recursive(&entry.path(), &child_dest)?;
        }
        Ok(())
    } else {
        fs::copy(src, dest).map(|_| ())
    }
}
