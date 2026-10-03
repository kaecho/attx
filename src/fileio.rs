//! Per-file replacement through held directory capabilities.
//!
//! Output batches stage all artifacts before committing. Backups retain the first
//! existing destination. A directory-wide writeback is not a filesystem transaction.

use anyhow::{Context, Result, bail};
use cap_std::{ambient_authority, fs::{Dir, OpenOptions as CapOpenOptions}};
use sha2::{Digest, Sha256};
use same_file::Handle;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

pub fn fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// OS locks release on process exit, including crashes.
pub struct WorkspaceLock { _file: File }

impl WorkspaceLock {
    pub fn acquire(workspace: &Path) -> Result<Self> {
        fs::create_dir_all(workspace)?;
        let path = workspace.join(".attx.lock");
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            bail!("workspace lock is a symlink: {}", path.display());
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)] {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        file.try_lock().with_context(|| format!("workspace {} is busy; wait for its other mutating command", workspace.display()))?;
        Ok(Self { _file: file })
    }
}

pub struct StagedFile {
    parent: Dir,
    parent_identity: Handle,
    name: OsString,
    temporary: OsString,
    destination: PathBuf,
}

fn open_private(parent: &Dir, name: &OsStr) -> Result<(OsString, File)> {
    let mut options = CapOpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    loop {
        let mut temporary = name.to_os_string();
        temporary.push(format!(".attxtmp-{}-{}", std::process::id(), NEXT_FILE.fetch_add(1, Ordering::Relaxed)));
        match parent.open_with(&temporary, &options) {
            Ok(file) => return Ok((temporary, file.into_std())),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).context("create private staged file"),
        }
    }
}

fn check_regular(parent: &Dir, name: &OsStr) -> Result<bool> {
    match parent.symlink_metadata(name) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => bail!("refusing non-regular artifact {}", name.to_string_lossy()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).context("inspect artifact"),
    }
}

fn backup_in(parent: &Dir, name: &OsStr) -> Result<bool> {
    if !check_regular(parent, name)? { return Ok(false); }
    let mut backup = name.to_os_string();
    backup.push(".attxbak");
    if check_regular(parent, &backup)? { return Ok(true); }
    let mut source = parent.open(name)?.into_std();
    let permissions = source.metadata()?.permissions();
    let (temporary, mut file) = open_private(parent, &backup)?;
    let result = (|| -> Result<()> {
        std::io::copy(&mut source, &mut file)?;
        file.set_permissions(permissions)?;
        file.sync_all()?;
        drop(file);
        match parent.hard_link(&temporary, parent, &backup) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                check_regular(parent, &backup)?;
                Ok(())
            }
            Err(e) => Err(e).context("publish complete first-write backup"),
        }
    })();
    let _ = parent.remove_file(&temporary);
    result?;
    Ok(true)
}

impl StagedFile {
    fn open_in(parent: Dir, destination: &Path) -> Result<(Self, File)> {
        let name = destination.file_name().context("output has no file name")?.to_os_string();
        check_regular(&parent, &name)?;
        let parent_identity = Handle::from_file(parent.try_clone()?.into_std_file())?;
        let (temporary, file) = open_private(&parent, &name)?;
        Ok((Self { parent, parent_identity, name, temporary, destination: destination.to_path_buf() }, file))
    }

    pub fn open(destination: &Path) -> Result<(Self, File)> {
        let parent = destination.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        Self::open_in(Dir::open_ambient_dir(parent, ambient_authority())?, destination)
    }

    fn finish(staged: Self, mut file: File, bytes: &[u8], permissions: Option<&Permissions>) -> Result<Self> {
        file.write_all(bytes).with_context(|| format!("stage {}", staged.destination.display()))?;
        let inherited = match permissions {
            Some(mode) => Some(mode.clone()),
            None if check_regular(&staged.parent, &staged.name)? => Some(staged.parent.open(&staged.name)?.into_std().metadata()?.permissions()),
            None => None,
        };
        if let Some(mode) = inherited { file.set_permissions(mode)?; }
        file.sync_all()?;
        Ok(staged)
    }

    pub fn new(destination: &Path, bytes: &[u8]) -> Result<Self> {
        Self::new_with_permissions(destination, bytes, None)
    }

    pub fn new_with_permissions(destination: &Path, bytes: &[u8], permissions: Option<&Permissions>) -> Result<Self> {
        let (staged, file) = Self::open(destination)?;
        Self::finish(staged, file, bytes, permissions)
    }

    /// Descendant traversal and later publication stay beneath a held input root.
    pub fn beneath(root: &Path, destination: &Path, bytes: &[u8], permissions: Option<&Permissions>) -> Result<Self> {
        let relative = destination.strip_prefix(root).context("output is outside input root")?;
        let parent = relative.parent().context("output has no parent")?;
        let mut directory = Dir::open_ambient_dir(root, ambient_authority())?;
        let mut source_directory = root.to_path_buf();
        for (depth, component) in parent.components().enumerate() {
            let Component::Normal(name) = component else { bail!("invalid output descendant path") };
            if depth > 0 { source_directory.push(name); }
            match directory.symlink_metadata(name) {
                Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
                Ok(_) => bail!("refusing non-directory or symlink output parent {}", name.to_string_lossy()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => directory.create_dir(name)?,
                Err(e) => return Err(e).context("inspect output directory"),
            }
            directory = directory.open_dir(name)?;
            let mode = fs::metadata(&source_directory)?.permissions();
            let current = directory.dir_metadata()?.permissions();
            let wanted = cap_std::fs::Permissions::from_std(mode);
            if current != wanted { directory.set_permissions(".", wanted)?; }
        }
        let (staged, file) = Self::open_in(directory, destination)?;
        Self::finish(staged, file, bytes, permissions)
    }


    fn verify_parent(&self) -> Result<()> {
        let parent = self.destination.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        if Handle::from_path(parent)? != self.parent_identity {
            bail!("output parent changed during staging: {}", parent.display());
        }
        Ok(())
    }
    pub fn ensure_backup(&self) -> Result<bool> {
        self.verify_parent()?;
        backup_in(&self.parent, &self.name)
    }

    pub fn commit(self) -> Result<()> {
        self.verify_parent()?;
        check_regular(&self.parent, &self.name)?;
        self.parent.rename(&self.temporary, &self.parent, &self.name)
            .with_context(|| format!("replace {}", self.destination.display()))?;
        Ok(())
    }

    pub fn commit_new(self) -> Result<()> {
        self.verify_parent()?;
        self.parent.hard_link(&self.temporary, &self.parent, &self.name)
            .with_context(|| format!("publish new {} without overwriting", self.destination.display()))?;
        Ok(())
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) { let _ = self.parent.remove_file(&self.temporary); }
}

pub fn ensure_backup(path: &Path) -> Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let dir = Dir::open_ambient_dir(parent, ambient_authority())?;
    backup_in(&dir, path.file_name().context("backup path has no name")?)?;
    Ok(())
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    StagedFile::new(path, bytes)?.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discarded_stage_leaves_original_and_backup_unchanged() {
        let root = crate::adapter::test_dir("discarded-stage");
        let path = root.join("source.txt");
        fs::write(&path, "original").unwrap();
        ensure_backup(&path).unwrap();
        drop(StagedFile::new(&path, b"translated").unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        assert_eq!(fs::read_to_string(root.join("source.txt.attxbak")).unwrap(), "original");
    }

    #[test]
    fn replacements_keep_first_backup_and_invalid_backup_blocks() {
        let root = crate::adapter::test_dir("first-backup");
        let path = root.join("source.txt");
        fs::write(&path, "original").unwrap();
        ensure_backup(&path).unwrap();
        write_atomic(&path, b"first translation").unwrap();
        ensure_backup(&path).unwrap();
        write_atomic(&path, b"second translation").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second translation");
        assert_eq!(fs::read_to_string(root.join("source.txt.attxbak")).unwrap(), "original");
        let blocked = root.join("blocked.txt");
        fs::write(&blocked, "unchanged").unwrap();
        fs::create_dir(root.join("blocked.txt.attxbak")).unwrap();
        assert!(ensure_backup(&blocked).is_err());
        assert_eq!(fs::read_to_string(blocked).unwrap(), "unchanged");
    }

    #[cfg(unix)]
    #[test]
    fn staging_backups_and_new_copies_keep_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = crate::adapter::test_dir("private-stage");
        let path = root.join("source.txt");
        fs::write(&path, "private original").unwrap();
        fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
        let (staged, file) = StagedFile::open(&path).unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        drop(file); drop(staged);
        ensure_backup(&path).unwrap();
        assert_eq!(fs::metadata(root.join("source.txt.attxbak")).unwrap().permissions().mode() & 0o777, 0o600);
        let output = root.join("translated-zh/copy.txt");
        StagedFile::beneath(&root, &output, b"private copy", Some(&fs::metadata(&path).unwrap().permissions())).unwrap().commit().unwrap();
        assert_eq!(fs::metadata(output).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn held_directory_prevents_symlink_redirect_after_staging() {
        use std::os::unix::fs::symlink;
        let root = crate::adapter::test_dir("held-parent");
        let outside = crate::adapter::test_dir("held-outside");
        let destination = root.join("translated-zh/result.txt");
        let stage = StagedFile::beneath(&root, &destination, b"translated", None).unwrap();
        fs::rename(root.join("translated-zh"), root.join("original-output-dir")).unwrap();
        symlink(&outside, root.join("translated-zh")).unwrap();
        assert!(stage.commit().is_err());
        assert!(!outside.join("result.txt").exists());
        assert!(!root.join("original-output-dir/result.txt").exists());
        assert!(StagedFile::beneath(&root, &destination, b"must not escape", None).is_err());
    }
}
