mod run;
mod tasks;
mod updates;

use std::{
    fs::{self, Metadata},
    io::{self, StdoutLock},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use clap::{command, Parser};
use crossterm::terminal;
use regex::Regex;
use updates::Updates;

const EXIT_NO_ABSOLUTE_PATH: i32 = 1;

fn main() -> io::Result<()> {
    let args = Args::parse();
    let current_dir = match args.dir {
        Some(dir) => {
            if args.dir_relative || dir.is_absolute() {
                dir
            } else {
                match fs::canonicalize(dir) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Error getting absolute path: {e}.");
                        std::process::exit(EXIT_NO_ABSOLUTE_PATH);
                    }
                }
            }
        }
        None => std::env::current_dir().unwrap_or(PathBuf::from("/")),
    };
    let mut share = Share {
        status: String::new(),
        tasks: vec![],
        active_instance: 0,
        total_instances: 1,
        stdout: io::stdout().lock(),
        size: terminal::size()?,
        terminal_command: std::env::var("TERM").unwrap_or("alacritty".to_string()),
        editor_command: std::env::var("EDITOR").unwrap_or("nano".to_string()),
        live_search: !args.no_live_search,
        info_what: vec![0, 1],
    };
    if args.check {
        eprintln!("Terminal: {}", share.terminal_command);
        eprintln!("Editor: {}", share.editor_command);
        return Ok(());
    }
    let mut instances = vec![TuiFile::new(current_dir)?];
    TuiFile::term_setup_no_redraw(&mut share)?;
    let mut redraw = true;
    loop {
        if instances.is_empty() {
            break;
        }
        if share.active_instance >= instances.len() {
            share.active_instance = instances.len() - 1;
        }
        share.total_instances = instances.len();
        let instance = &mut instances[share.active_instance];
        if redraw {
            instance.updates.request_clear();
            instance.updates.request_redraw();
            if instance.active {
                share.status = format!("{}", share.active_instance);
            }
        }
        let cmd = instance.run(&mut share)?;
        redraw = match cmd {
            AppCmd::Quit => break,
            AppCmd::CloseInstance => {
                instances.remove(share.active_instance);
                if share.active_instance > 0 {
                    share.active_instance -= 1;
                }
                true
            }
            AppCmd::NextInstance => {
                if share.active_instance + 1 < instances.len() {
                    share.active_instance += 1;
                }
                true
            }
            AppCmd::PrevInstance => {
                if share.active_instance > 0 {
                    share.active_instance -= 1;
                }
                true
            }
            AppCmd::AddInstance(new) => {
                share.active_instance += 1;
                instances.insert(share.active_instance, new);
                true
            }
            AppCmd::CopyTo(destination) => {
                instance.updates.request_redraw_infobar();
                let src = instances
                    .iter()
                    .filter(|v| v.active)
                    .map(|v| {
                        (
                            v.current_dir.clone(),
                            v.dir_content
                                .iter()
                                .filter(|e| e.selected)
                                .filter_map(|e| {
                                    Some((
                                        e.path.strip_prefix(&v.current_dir).ok()?.to_owned(),
                                        e.rel_depth == v.scan_files_max_depth,
                                    ))
                                })
                                .collect(),
                        )
                    })
                    .collect();
                tasks::task_copy(src, destination, &mut share);
                false
            }
            AppCmd::TaskFinished => {
                for i in &mut instances {
                    i.updates.request_rescan_files();
                }
                false
            }
        };
    }
    TuiFile::term_reset(&mut share)?;
    Ok(())
}

/// TUI file explorer. Long Help is available with --help.
///
/// Controls:
/// - Ctrl+Up/K => previous
/// - Ctrl+Down/J => next
/// - Ctrl+Left/H => close
/// - Ctrl+Right/L => duplicate
/// Files:
/// - Up/K or Down/J => move selection
/// - Left/H => go to parent directory
/// - Right/L => go into selected entry
/// - A => Alternate selection (toggle All)
/// - S => Select or toggle current
/// - D => Deselect all
/// - F => focus Find/Filter bar
/// - M => set Mode based on Find/Filter bar ((t/b)[seconds])
/// - N => New directory from search text
/// - C => Copy selected files to this directory.
/// - R => Remove selected files and directories non-recursively
/// - P -> set Permissions (mode taken as base-8 number from find/filter bar text)
/// - O -> set Owner (and group - TODO!)
/// - 1-9 or 0 => set recursive depth limit (0 = infinite)
/// - T => open terminal here ($TERM)
/// - E => open in editor ($EDITOR <file/dir>)
/// Find/Filter Bar:
/// - Esc: back and discard
/// - Enter: back and apply
/// - Backspace: delete
/// - type to enter search regex
#[derive(Parser, Debug)]
#[command(version, verbatim_doc_comment)]
struct Args {
    /// the directory you want to view.
    dir: Option<PathBuf>,
    /// skips converting the 'dir' argument to an absolute path.
    /// this causes issues when trying to view parent directories
    /// but may be necessary if tuifile doesn't start.
    #[arg(long)]
    dir_relative: bool,
    /// performs some checks and prints results.
    #[arg(long)]
    check: bool,
    /// disables live search, only filtering the file list when enter is pressed.
    #[arg(long)]
    no_live_search: bool,
}

struct Share {
    status: String,
    tasks: Vec<BackgroundTask>,
    active_instance: usize,
    total_instances: usize,
    size: (u16, u16),
    stdout: StdoutLock<'static>,
    //
    live_search: bool,
    terminal_command: String,
    editor_command: String,
    /// 0: size
    /// 1: mode (permissions)
    info_what: Vec<u32>,
}
impl Share {
    /// returns Some if any task has finished.
    /// returns Some(true) if at least one of these tasks may have altered files.
    /// (this should trigger a rescan)
    fn check_bgtasks(&mut self) -> Option<bool> {
        for (i, task) in self.tasks.iter_mut().enumerate() {
            if task.thread.is_finished() {
                return Some(self.tasks.remove(i).rescan_after);
            }
        }
        None
    }
}
struct BackgroundTask {
    status: Arc<Mutex<String>>,
    thread: JoinHandle<Result<(), String>>,
    rescan_after: bool,
}
impl BackgroundTask {
    pub fn new(
        text: String,
        func: impl FnOnce(Arc<Mutex<String>>) -> Result<(), String> + Send + 'static,
        rescan_after: bool,
    ) -> Self {
        let status = Arc::new(Mutex::new(text));
        Self {
            status: Arc::clone(&status),
            thread: std::thread::spawn(move || func(status)),
            rescan_after,
        }
    }
}
struct TuiFile {
    active: bool,
    updates: u32,
    current_dir: PathBuf,
    dir_content: Vec<DirContent>,
    dir_content_len: usize,
    dir_content_builder_task: Option<Arc<Mutex<Option<Result<Vec<DirContent>, String>>>>>,
    scroll: usize,
    current_index: usize,
    focus: Focus,
    scan_files_max_depth: usize,
    files_status_is_special: bool,
    files_status: String,
    search_text: String,
    search_regex: Option<Regex>,
    last_drawn_files_height: usize,
    last_drawn_files_count: usize,
    last_files_max_scroll: usize,
    after_rescanning_files: Vec<Box<dyn FnOnce(&mut Self)>>,
    scan_files_mode: ScanFilesMode,
}
#[derive(Clone)]
enum ScanFilesMode {
    /// file-scanning blocks the main thread.
    /// prevents flickering.
    Blocking,
    /// file-scanning doesn't block the main thread.
    /// leads to flickering as the file list appears empty until the thread finishes.
    Threaded,
    /// file-scanning blocks the main thread for up to _ seconds.
    /// after the timeout is reached, file scanning is stopped.
    /// can lead to incomplete file lists.
    Timeout(f32),
    /// file-scanning blocks the main thread for up to _ seconds.
    /// after the timeout is reached, file-scanning will restart on a thread.
    /// prevents flickering but will scan the first files twice if the timeout is reached.
    TimeoutThenThreaded(f32),
}
impl Default for ScanFilesMode {
    fn default() -> Self {
        Self::Blocking
    }
}
#[derive(Clone)]
struct DirContent {
    path: PathBuf,
    name: String,
    name_charlen: usize,
    rel_depth: usize,
    passes_filter: bool,
    selected: bool,
    info: String,
    more: DirContentType,
}
#[derive(Clone)]
enum DirContentType {
    /// Couldn't get more info on this entry
    Err(String),
    Dir {
        metadata: Metadata,
    },
    File {
        metadata: Metadata,
    },
    Symlink {
        metadata: Metadata,
    },
}
#[derive(Clone)]
enum Focus {
    Files,
    SearchBar,
}
enum AppCmd {
    Quit,
    CloseInstance,
    NextInstance,
    PrevInstance,
    AddInstance(TuiFile),
    CopyTo(PathBuf),
    TaskFinished,
}
impl TuiFile {
    pub fn clone(&self) -> Self {
        Self {
            active: self.active,
            updates: 0,
            current_dir: self.current_dir.clone(),
            dir_content: self.dir_content.clone(),
            dir_content_len: self.dir_content_len,
            dir_content_builder_task: None,
            scroll: self.scroll,
            current_index: self.current_index,
            focus: self.focus.clone(),
            scan_files_max_depth: self.scan_files_max_depth,
            files_status_is_special: self.files_status_is_special,
            files_status: self.files_status.clone(),
            search_text: self.search_text.clone(),
            search_regex: self.search_regex.clone(),
            last_drawn_files_height: self.last_drawn_files_height,
            last_drawn_files_count: self.last_drawn_files_count,
            last_files_max_scroll: self.last_files_max_scroll,
            after_rescanning_files: vec![],
            scan_files_mode: ScanFilesMode::default(),
        }
    }
    pub fn new(current_dir: PathBuf) -> io::Result<Self> {
        // state
        let (_width, _height) = terminal::size()?;
        let updates = u32::MAX;
        Ok(Self {
            active: true,
            updates,
            current_dir,
            dir_content: vec![],
            dir_content_len: 0,
            dir_content_builder_task: None,
            scroll: 0,
            current_index: 0,
            focus: Focus::Files,
            scan_files_max_depth: 0,
            files_status_is_special: false,
            files_status: String::new(),
            search_text: String::new(),
            search_regex: None,
            last_drawn_files_height: 0,
            last_drawn_files_count: 0,
            last_files_max_scroll: 0,
            after_rescanning_files: vec![],
            scan_files_mode: ScanFilesMode::default(),
        })
    }
    fn set_current_index(&mut self, mut i: usize) {
        if i >= self.dir_content.len() {
            i = self.dir_content.len().saturating_sub(1);
        }
        if i == self.current_index {
            return;
        }
        if i < self.scroll {
            self.scroll = i;
            self.updates.request_redraw_filelist();
        }
        if i >= self.scroll + self.last_drawn_files_height {
            self.scroll = 1 + i - self.last_drawn_files_height;
            self.updates.request_redraw_filelist();
        }
        self.updates.request_move_cursor();
        // self.updates.request_redraw_filelist();
        self.current_index = i;
    }
    /// starting from `start`, checks all indices until it finds a visible entry or there are no more entries.
    /// If an entry was found, the current_index will be set to that entry.
    fn set_current_index_to_visible(&mut self, start: usize, inc: bool) {
        let mut i = start;
        loop {
            if self.dir_content.get(i).is_some_and(|e| e.passes_filter) {
                self.set_current_index(i);
                return;
            }
            if inc {
                i += 1;
                if i >= self.dir_content.len() {
                    break;
                }
            } else if i > 0 {
                i -= 1;
            } else {
                break;
            }
        }
    }
    fn request_rescan_files_then_select(
        &mut self,
        find_by: impl FnMut(&DirContent) -> bool + 'static,
    ) {
        self.updates.request_rescan_files();
        self.after_rescanning_files.push(Box::new(move |s| {
            if let Some(i) = s.dir_content.iter().position(find_by) {
                s.set_current_index(i)
            } else {
                s.updates.request_reset_current_index();
            }
        }));
    }
    fn request_rescan_files_then_select_by_name(&mut self, name: String) {
        self.request_rescan_files_then_select(move |e| {
            e.name == name || e.name.ends_with('/') && e.name[..e.name.len() - 1] == name
        });
    }
    fn request_rescan_files_then_select_current_again(&mut self) {
        if let Some(c) = self.dir_content.get(self.current_index) {
            self.request_rescan_files_then_select_by_name(c.name.clone());
        } else {
            self.updates.request_rescan_files();
        }
    }
}
