use std::{
    collections::HashSet,
    fs, io,
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
};

use crate::{BackgroundTask, Share};

pub(crate) fn task_copy(
    src: Vec<(PathBuf, Vec<(PathBuf, bool)>)>,
    target: PathBuf,
    share: &mut Share,
) {
    share.tasks.push(BackgroundTask::new(
        "cp".to_string(),
        move |status| {
            let mut total: usize = src.iter().map(|v| v.1.len()).sum();
            for (parent, rel_paths) in src {
                let mut created: HashSet<PathBuf> = HashSet::new();
                for (rel_path, copy_recursive) in rel_paths {
                    total = total.saturating_sub(1);
                    {
                        let s = format!("cp {total}");
                        *status.lock().unwrap() = s;
                    }
                    let file_from = parent.join(&rel_path);
                    let is_dir = file_from.is_dir();
                    let file_to = if let Some(parent) = rel_path.parent() {
                        let mut p = PathBuf::new();
                        for c in parent.components() {
                            p.push(c);
                            if !created.contains(&p) {
                                p.pop();
                                break;
                            }
                        }
                        target.join(&p).join(rel_path.file_name().unwrap())
                    } else {
                        target.join(&rel_path)
                    };
                    if is_dir {
                        copy_dir(file_from, file_to, copy_recursive);
                        created.insert(rel_path);
                    } else {
                        fs::copy(&file_from, &file_to);
                    }
                }
            }
            Ok(())
        },
        true,
    ));
}
fn copy_dir(
    file_from: impl AsRef<Path>,
    file_to: impl AsRef<Path>,
    recursive: bool,
) -> io::Result<()> {
    fs::create_dir(&file_to)?;
    if recursive {
        if let Ok(e) = fs::read_dir(file_from) {
            for e in e {
                if let Ok(e) = e {
                    let p = e.path();
                    let t = file_to.as_ref().join(e.file_name());
                    if p.is_dir() {
                        copy_dir(p, t, recursive);
                    } else {
                        fs::copy(&p, &t);
                    }
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn task_del(paths: Vec<PathBuf>, share: &mut Share) {
    let mut total: usize = paths.len();
    share.tasks.push(BackgroundTask::new(
        format!("rm {total}"),
        move |status| {
            for path in paths {
                {
                    total -= 1;
                    let s = format!("rm {total}");
                    *status.lock().unwrap() = s;
                }
                if path.is_dir() {
                    fs::remove_dir(path);
                } else {
                    fs::remove_file(path);
                }
            }
            Ok(())
        },
        true,
    ));
}
pub(crate) fn task_chmod(paths: Vec<PathBuf>, mode: u32, share: &mut Share) {
    let mut total = paths.len();
    share.tasks.push(BackgroundTask::new(
        format!("chmod {total}"),
        move |status| {
            for path in paths {
                {
                    total -= 1;
                    let s = format!("chmod {total}");
                    *status.lock().unwrap() = s;
                }
                fs::set_permissions(path, fs::Permissions::from_mode(mode));
            }
            Ok(())
        },
        true,
    ));
}
