//! Application-owned ciphertext files only. User file names never become cache paths.
use crate::AppError;
use onepaste_core::attachment::Attachment;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub(crate) const DIRECTORY: &str = "attachments";
fn storage(_: std::io::Error) -> AppError {
    AppError::Attachment("attachment_storage")
}
fn directory(path: &Path) -> Result<(), AppError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(AppError::Attachment("attachment_storage")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(path).map_err(storage)
        }
        Err(error) => Err(storage(error)),
    }
}
pub(super) fn blob_dir(root: &Path, blob: &str) -> PathBuf {
    root.join(DIRECTORY).join(blob)
}
pub(super) fn create(root: &Path, blob: &str) -> Result<PathBuf, AppError> {
    onepaste_core::id::validate_id(blob)
        .map_err(|_| AppError::Attachment("attachment_metadata"))?;
    directory(&root.join(DIRECTORY))?;
    let path = blob_dir(root, blob);
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&path).map_err(storage)?;
    Ok(path)
}
pub(super) fn ensure(root: &Path, blob: &str) -> Result<PathBuf, AppError> {
    onepaste_core::id::validate_id(blob)
        .map_err(|_| AppError::Attachment("attachment_metadata"))?;
    directory(&root.join(DIRECTORY))?;
    let path = blob_dir(root, blob);
    directory(&path)?;
    Ok(path)
}
fn existing(path: &Path) -> Result<(), AppError> {
    let meta = std::fs::symlink_metadata(path).map_err(storage)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(AppError::Attachment("attachment_storage"));
    }
    Ok(())
}
pub(super) fn read(root: &Path, desc: &Attachment, index: u32) -> Result<Vec<u8>, AppError> {
    existing(&root.join(DIRECTORY))?;
    existing(&blob_dir(root, &desc.blob_id))?;
    let path = blob_dir(root, &desc.blob_id).join(format!("{index}.bin"));
    let meta = std::fs::symlink_metadata(&path).map_err(storage)?;
    let max = onepaste_core::attachment::MAX_SEALED_CHUNK;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > max as u64 {
        return Err(AppError::Attachment("attachment_integrity"));
    }
    let mut data = Vec::new();
    std::fs::File::open(path)
        .map_err(storage)?
        .take(max as u64 + 1)
        .read_to_end(&mut data)
        .map_err(storage)?;
    if data.len() > max {
        return Err(AppError::Attachment("attachment_integrity"));
    }
    Ok(data)
}
pub(super) fn complete(root: &Path, desc: &Attachment) -> bool {
    if existing(&root.join(DIRECTORY)).is_err() || existing(&blob_dir(root, &desc.blob_id)).is_err()
    {
        return false;
    }
    (0..desc.chunks).all(|index| {
        std::fs::symlink_metadata(blob_dir(root, &desc.blob_id).join(format!("{index}.bin")))
            .map(|meta| {
                meta.is_file()
                    && !meta.file_type().is_symlink()
                    && meta.len()
                        == desc
                            .plain_chunk_len(index)
                            .unwrap_or(usize::MAX)
                            .saturating_add(40) as u64
            })
            .unwrap_or(false)
    })
}
pub(super) fn write(
    root: &Path,
    desc: &Attachment,
    index: u32,
    data: &[u8],
) -> Result<(), AppError> {
    if data.len()
        != desc
            .plain_chunk_len(index)
            .map_err(|_| AppError::Attachment("attachment_metadata"))?
            + 40
    {
        return Err(AppError::Attachment("attachment_integrity"));
    }
    let directory = ensure(root, &desc.blob_id)?;
    let temporary = directory.join(format!(
        "{index}-{}.tmp",
        onepaste_core::id::new_id().map_err(|_| AppError::Attachment("attachment_storage"))?
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary).map_err(storage)?;
        file.write_all(data).map_err(storage)?;
        file.sync_all().map_err(storage)?;
        drop(file);
        std::fs::rename(&temporary, directory.join(format!("{index}.bin"))).map_err(storage)?;
        #[cfg(unix)]
        {
            std::fs::File::open(&directory)
                .map_err(storage)?
                .sync_all()
                .map_err(storage)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
pub(super) fn remove(root: &Path, blob: &str) {
    if onepaste_core::id::validate_id(blob).is_ok() {
        let _ = std::fs::remove_dir_all(blob_dir(root, blob));
    }
}
pub(super) fn prune(
    root: &Path,
    references: &std::collections::HashSet<String>,
) -> Result<(), AppError> {
    let path = root.join(DIRECTORY);
    if !path.exists() {
        return Ok(());
    }
    directory(&path)?;
    for entry in std::fs::read_dir(path).map_err(storage)?.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if onepaste_core::id::validate_id(&name).is_ok()
            && !references.contains(&name)
            && entry.file_type().map_err(storage)?.is_dir()
        {
            std::fs::remove_dir_all(entry.path()).map_err(storage)?;
        }
    }
    Ok(())
}
