use crossterm::event::{poll, read, Event, KeyCode, KeyModifiers};
use crossterm::style::{Attribute, Color, Stylize};
use crossterm::{cursor, queue, style, terminal, ExecutableCommand};
use regex::RegexBuilder;

use crate::updates::Updates;
use crate::{tasks, AppCmd, BackgroundTask, DirContent, DirContentType, Focus, Share};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;
use std::{fs, io};

use crate::TuiFile;

const BYTE_UNITS: [&'static str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

impl TuiFile {
    pub fn term_setup(&mut self, share: &mut Share) -> io::Result<()> {
        self.updates.request_redraw();
        Self::term_setup_no_redraw(share)
    }
    pub fn term_setup_no_redraw(share: &mut Share) -> io::Result<()> {
        share.stdout.execute(terminal::EnterAlternateScreen)?;
        terminal::enable_raw_mode()?;
        Ok(())
    }
    pub fn term_reset(share: &mut Share) -> io::Result<()> {
        terminal::disable_raw_mode()?;
        share.stdout.execute(terminal::LeaveAlternateScreen)?;
        Ok(())
    }
    pub fn run(&mut self, share: &mut Share) -> io::Result<AppCmd> {
        loop {
            if share.check_bgtasks() {
                return Ok(AppCmd::TaskFinished);
            }
            // rescan files if necessary
            if self.updates.rescan_files() {
                self.updates.dont_rescan_files();
                self.updates.request_filter_files();
                self.files_status_is_special = false;
                self.dir_content.clear();
                get_files(self, self.current_dir.clone(), 0);
                fn get_files(s: &mut TuiFile, dir: PathBuf, depth: usize) {
                    match fs::read_dir(&dir) {
                        Err(e) => {
                            if depth == 0 {
                                s.dir_content = vec![];
                                s.files_status = format!("{e}");
                                s.files_status_is_special = true;
                            }
                        }
                        Ok(files) => {
                            for entry in files {
                                if let Ok(entry) = entry {
                                    let mut name = entry.file_name().to_string_lossy().into_owned();
                                    let metadata = entry.metadata();
                                    let p = entry.path();
                                    let more = match metadata {
                                        Err(e) => DirContentType::Err(e.to_string()),
                                        Ok(metadata) => {
                                            if metadata.is_symlink() {
                                                DirContentType::Symlink { metadata }
                                            } else if metadata.is_file() {
                                                DirContentType::File {
                                                    size: {
                                                        let mut bytes = metadata.len();
                                                        let mut i = 0;
                                                        loop {
                                                            if bytes < 1024
                                                                || i + 1 >= BYTE_UNITS.len()
                                                            {
                                                                break format!(
                                                                    "{bytes}{}",
                                                                    BYTE_UNITS[i]
                                                                );
                                                            } else {
                                                                i += 1;
                                                                // divide by 1024 but cooler
                                                                bytes >>= 10;
                                                            }
                                                        }
                                                    },
                                                    metadata,
                                                }
                                            } else if metadata.is_dir() {
                                                DirContentType::Dir { metadata }
                                            } else {
                                                DirContentType::Err(format!(
                                                    "not a file, dir or symlink"
                                                ))
                                            }
                                        }
                                    };
                                    if let DirContentType::Dir { .. } = more {
                                        name.push('/');
                                    }
                                    s.dir_content.push(DirContent {
                                        entry: Rc::new(entry),
                                        name_charlen: name.chars().count(),
                                        name,
                                        rel_depth: depth,
                                        passes_filter: true,
                                        selected: false,
                                        more,
                                    });
                                    if depth < s.scan_files_max_depth {
                                        get_files(s, p, depth + 1);
                                    }
                                }
                            }
                        }
                    }
                }
                if self.current_index >= self.dir_content.len() {
                    self.current_index = self.dir_content.len().saturating_sub(1);
                }
                if !self.after_rescanning_files.is_empty() {
                    for func in std::mem::replace(&mut self.after_rescanning_files, vec![]) {
                        func(self);
                    }
                }
            }
            if self.updates.reset_search() {
                self.updates.dont_reset_search();
                if !self.search_text.is_empty() {
                    self.search_text.clear();
                    self.search_regex = None;
                    self.updates.request_redraw_searchbar();
                }
            }
            if self.updates.filter_files() {
                if self.search_regex.is_none() && !self.search_text.is_empty() {
                    self.search_regex = RegexBuilder::new(&self.search_text)
                        .case_insensitive(true)
                        .build()
                        .ok();
                }
                self.updates.dont_filter_files();
                self.updates.request_redraw_filelist();
                if let Some(regex) = &self.search_regex {
                    self.dir_content_len = 0;
                    for entry in &mut self.dir_content {
                        entry.passes_filter = regex.is_match(&entry.name);
                        if entry.passes_filter {
                            self.dir_content_len += 1;
                        }
                    }
                } else {
                    for entry in &mut self.dir_content {
                        entry.passes_filter = true;
                    }
                    self.dir_content_len = self.dir_content.len();
                }
                if !self.files_status_is_special {
                    self.files_status = match (
                        self.dir_content_len != self.dir_content.len(),
                        self.dir_content.len() == 1,
                    ) {
                        (false, false) => format!("{} entries", self.dir_content_len),
                        (false, true) => format!("1 entry"),
                        (true, false) => format!(
                            "{} of {} entries",
                            self.dir_content_len,
                            self.dir_content.len()
                        ),
                        (true, true) => format!("{} of 1 entry", self.dir_content_len),
                    };
                    if self.scan_files_max_depth > 0 {
                        if let Some(v) = self.scan_files_max_depth.checked_add(1) {
                            self.files_status.push_str(&format!(" ({v} layers)",));
                        } else {
                            self.files_status.push_str(&format!(" (recursive)",));
                        }
                    }
                }
            }
            if self.updates.reset_current_index() {
                self.updates.dont_reset_current_index();
                self.set_current_index_to_visible(0, true);
            }
            // draw tui
            if share.size.0 > 0 && share.size.1 > 0 {
                if self.updates.clear() {
                    self.updates.dont_clear();
                    self.updates.request_move_cursor();
                    queue!(share.stdout, terminal::Clear(terminal::ClearType::All))?;
                }
                if self.updates.redraw_infobar() {
                    self.updates.dont_redraw_infobar();
                    self.updates.request_move_cursor();
                    let mut pathstring = share.status.clone();
                    if share.tasks.len() > 0 {
                        self.updates.request_redraw_infobar();
                        for task in share.tasks.iter() {
                            pathstring.push_str(" | ");
                            pathstring.push_str(task.status.lock().unwrap().as_str());
                        }
                    }
                    pathstring.push_str(" - ");
                    if share.size.0 as usize > pathstring.len() {
                        let mut pathchars = Vec::with_capacity(self.current_dir.as_os_str().len());
                        let mut maxlen = share.size.0 as usize - pathstring.len();
                        for ch in self
                            .current_dir
                            .as_os_str()
                            .to_string_lossy()
                            .to_string()
                            .chars()
                            .rev()
                        {
                            if maxlen > 0 {
                                pathchars.push(ch);
                                maxlen -= 1;
                            }
                        }
                        pathstring.extend(pathchars.into_iter().rev());
                        pathstring.reserve_exact(maxlen as usize);
                        for _ in 0..maxlen {
                            pathstring.push(' ');
                        }
                        queue!(
                            share.stdout,
                            cursor::MoveTo(0, 0),
                            style::PrintStyledContent(
                                pathstring
                                    .with(Color::Cyan)
                                    .attribute(Attribute::Underlined)
                            )
                        )?;
                    }
                }
                if self.updates.redraw_filelist() {
                    self.updates.dont_redraw_filelist();
                    self.updates.request_move_cursor();
                    self.last_drawn_files_height = share.size.1.saturating_sub(3) as _;
                    let mut status = format!(" {}", self.files_status);
                    while status.len() < share.size.0 as usize {
                        status.push(' ');
                    }
                    queue!(
                        share.stdout,
                        cursor::MoveTo(0, 1),
                        style::PrintStyledContent(status.attribute(Attribute::Italic)),
                    )?;
                    self.last_files_max_scroll = self
                        .dir_content_len
                        .saturating_sub(self.last_drawn_files_height);
                    let scrollbar_where = if self.last_files_max_scroll > 0 {
                        Some(
                            self.last_drawn_files_height.saturating_sub(1) * self.scroll
                                / self.last_files_max_scroll,
                        )
                    } else {
                        None
                    };
                    let mut drawn_files = 0;
                    for (line, entry) in self
                        .dir_content
                        .iter()
                        .skip(self.scroll)
                        .filter(|e| e.passes_filter)
                        .take(self.last_drawn_files_height)
                        .enumerate()
                    {
                        drawn_files += 1;
                        let (mut text, mut text_charlen) = ("- ".to_string(), 2);
                        for _ in 0..entry.rel_depth {
                            text.push_str("  | ");
                        }
                        text_charlen += entry.rel_depth * 4;
                        let endchar = if let Some(sb_where) = scrollbar_where {
                            if line == sb_where {
                                '#'
                            } else {
                                '|'
                            }
                        } else {
                            ' '
                        };
                        let styled = match &entry.more {
                            DirContentType::Err(e) => {
                                text.push_str(&entry.name);
                                text_charlen += entry.name_charlen;
                                while text_charlen + 9 > share.size.0 as usize {
                                    text.pop();
                                    text_charlen -= 1;
                                }
                                text.push_str(" - Err: ");
                                text_charlen += 8;
                                for ch in e.chars() {
                                    if ch == '\n' || ch == '\r' {
                                        continue;
                                    }
                                    if text_charlen >= share.size.0 as usize {
                                        break;
                                    }
                                    text_charlen += 1;
                                    text.push(ch);
                                }
                                // make text_charlen 1 too large (for the endchar)
                                text_charlen += 1;
                                while text_charlen < share.size.0 as _ {
                                    text.push(' ');
                                    text_charlen += 1;
                                }
                                text.push(endchar);
                                vec![text.red()]
                            }
                            DirContentType::Dir { metadata } => {
                                let filenamelen = share.size.0 as usize - 2 - text_charlen;
                                if entry.name_charlen < filenamelen {
                                    text.push_str(&entry.name);
                                    for _ in 0..(filenamelen - entry.name_charlen) {
                                        text.push(' ');
                                    }
                                } else if entry.name_charlen == filenamelen {
                                    text.push_str(&entry.name);
                                } else {
                                    // the new length is the old length minus the combined length of the characters we want to cut off
                                    let i = entry.name.len()
                                        - entry
                                            .name
                                            .chars()
                                            .rev()
                                            .take(entry.name_charlen - filenamelen)
                                            .map(|char| char.len_utf8())
                                            .sum::<usize>();
                                    text.push_str(&entry.name[0..i.saturating_sub(3)]);
                                    text.push_str("...");
                                }
                                text.push(' ');
                                text.push(endchar);
                                vec![text.stylize()]
                            }
                            DirContentType::File { size, metadata } => {
                                let filenamelen =
                                    share.size.0 as usize - 3 - text_charlen - size.chars().count();
                                if entry.name_charlen < filenamelen {
                                    text.push_str(&entry.name);
                                    for _ in 0..(filenamelen - entry.name_charlen) {
                                        text.push(' ');
                                    }
                                } else if entry.name_charlen == filenamelen {
                                    text.push_str(&entry.name);
                                } else {
                                    // the new length is the old length minus the combined length of the characters we want to cut off
                                    let i = entry.name.len()
                                        - entry
                                            .name
                                            .chars()
                                            .rev()
                                            .take(entry.name_charlen - filenamelen)
                                            .map(|char| char.len_utf8())
                                            .sum::<usize>();
                                    text.push_str(&entry.name[0..i.saturating_sub(3)]);
                                    text.push_str("...");
                                }
                                text.push(' ');
                                text.push_str(&size);
                                text.push(' ');
                                text.push(endchar);
                                vec![text.stylize()]
                            }
                            DirContentType::Symlink { metadata } => {
                                let filenamelen = share.size.0 as usize - 2 - text_charlen;
                                if entry.name_charlen < filenamelen {
                                    text.push_str(&entry.name);
                                    for _ in 0..(filenamelen - entry.name_charlen) {
                                        text.push(' ');
                                    }
                                } else if entry.name_charlen == filenamelen {
                                    text.push_str(&entry.name);
                                } else {
                                    // the new length is the old length minus the combined length of the characters we want to cut off
                                    let i = entry.name.len()
                                        - entry
                                            .name
                                            .chars()
                                            .rev()
                                            .take(entry.name_charlen - filenamelen)
                                            .map(|char| char.len_utf8())
                                            .sum::<usize>();
                                    text.push_str(&entry.name[0..i.saturating_sub(3)]);
                                    text.push_str("...");
                                }
                                text.push(' ');
                                text.push(endchar);
                                vec![text.italic()]
                            }
                        };
                        queue!(share.stdout, cursor::MoveToNextLine(1))?;
                        for mut s in styled {
                            if entry.selected {
                                s = s.bold();
                            }
                            queue!(share.stdout, style::PrintStyledContent(s))?;
                        }
                    }
                    let empty_lines = self.last_drawn_files_count.saturating_sub(drawn_files);
                    self.last_drawn_files_count = drawn_files;
                    let empty_line = " ".repeat(share.size.0 as _);
                    for _ in 0..empty_lines {
                        queue!(
                            share.stdout,
                            cursor::MoveToNextLine(1),
                            style::PrintStyledContent(empty_line.as_str().stylize())
                        )?;
                    }
                }
                if self.updates.redraw_searchbar() {
                    self.updates.dont_redraw_searchbar();
                    self.updates.request_move_cursor();
                    let mut text = if self.search_text.len() > share.size.0 as _ {
                        self.search_text[(self.search_text.len() - share.size.0 as usize)..]
                            .to_string()
                    } else {
                        self.search_text.clone()
                    };
                    while text.len() < share.size.0 as _ {
                        text.push(' ');
                    }
                    queue!(
                        share.stdout,
                        cursor::MoveTo(0, share.size.1 - 1),
                        style::PrintStyledContent(text.underlined())
                    );
                }
                if self.updates.move_cursor() {
                    self.updates.dont_move_cursor();
                    match self.focus {
                        Focus::Files => {
                            if self
                                .dir_content
                                .get(self.current_index)
                                .is_some_and(|e| e.passes_filter)
                            {
                                let height = self
                                    .dir_content
                                    .iter()
                                    .skip(self.scroll)
                                    .take(self.current_index.saturating_sub(self.scroll))
                                    .filter(|e| e.passes_filter)
                                    .count();
                                if height < self.last_drawn_files_height {
                                    queue!(share.stdout, cursor::MoveTo(0, 2 + height as u16))?;
                                } else {
                                    queue!(share.stdout, cursor::MoveTo(0, 1))?;
                                }
                            } else {
                                queue!(share.stdout, cursor::MoveTo(0, 1))?;
                            }
                        }
                        Focus::SearchBar => {
                            queue!(
                                share.stdout,
                                cursor::MoveTo(self.search_text.len() as _, share.size.1 - 1)
                            )?;
                        }
                    }
                }
            }
            // end of draw
            share.stdout.flush()?;
            // events
            if poll(Duration::from_millis(100))? {
                match read()? {
                    Event::FocusGained => {}
                    Event::FocusLost => {}
                    Event::Mouse(e) => match (e.kind, e.column, e.row, e.modifiers) {
                        _ => {}
                    },
                    Event::Key(e) => match (&self.focus, e.code) {
                        // - - - Global - - -
                        // Ctrl+Left/H -> Close
                        (_, KeyCode::Left | KeyCode::Char('h'))
                            if e.modifiers == KeyModifiers::CONTROL =>
                        {
                            return Ok(AppCmd::CloseInstance);
                        }
                        // Ctrl+Right/L -> Duplicate
                        (_, KeyCode::Right | KeyCode::Char('l'))
                            if e.modifiers == KeyModifiers::CONTROL =>
                        {
                            return Ok(AppCmd::AddInstance(self.clone()));
                        }
                        // Ctrl+Up/K -> Prev
                        (_, KeyCode::Up | KeyCode::Char('k'))
                            if e.modifiers == KeyModifiers::CONTROL =>
                        {
                            return Ok(AppCmd::PrevInstance);
                        }
                        // Ctrl+Down/J -> Next
                        (_, KeyCode::Down | KeyCode::Char('j'))
                            if e.modifiers == KeyModifiers::CONTROL =>
                        {
                            return Ok(AppCmd::NextInstance);
                        }
                        // - - - Files - - -
                        // Down/J -> Down
                        (Focus::Files, KeyCode::Down | KeyCode::Char('j')) => {
                            self.set_current_index_to_visible(self.current_index + 1, true)
                        }
                        // Up/K -> Up
                        (Focus::Files, KeyCode::Up | KeyCode::Char('k')) => {
                            if self.current_index > 0 {
                                self.set_current_index_to_visible(self.current_index - 1, false)
                            }
                        }
                        // Left/H -> Leave Directory
                        (Focus::Files, KeyCode::Left | KeyCode::Char('h')) => {
                            // leave directory
                            if let Some(this_dir) = self
                                .current_dir
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                            {
                                self.current_dir.pop();
                                self.updates.request_redraw_infobar();
                                self.request_rescan_files_then_select_by_name(this_dir);
                            }
                        }
                        // Right/L -> Enter Directory
                        (Focus::Files, KeyCode::Right | KeyCode::Char('l')) => {
                            // descend into directory
                            if let Some(entry) = self.dir_content.get(self.current_index) {
                                self.current_dir = entry.entry.path();
                                self.updates = u32::MAX;
                            }
                        }
                        // A -> Select All
                        (Focus::Files, KeyCode::Char('a')) => {
                            self.updates.request_redraw_filelist();
                            for e in &mut self.dir_content {
                                if e.passes_filter {
                                    e.selected = !e.selected;
                                }
                            }
                        }
                        // S -> Toggle Select
                        (Focus::Files, KeyCode::Char('s')) => {
                            self.updates.request_redraw_filelist();
                            if let Some(e) = self.dir_content.get_mut(self.current_index) {
                                e.selected = !e.selected;
                            }
                        }
                        // D -> Deselect All
                        (Focus::Files, KeyCode::Char('d')) => {
                            self.updates.request_redraw_filelist();
                            for e in &mut self.dir_content {
                                if e.passes_filter {
                                    e.selected = false;
                                }
                            }
                        }
                        (Focus::Files, KeyCode::Char('f')) => {
                            self.focus = Focus::SearchBar;
                            self.updates.request_move_cursor();
                        }
                        // N -> New Directory
                        (Focus::Files, KeyCode::Char('n')) => {
                            let dir = self.current_dir.join(&self.search_text);
                            if fs::create_dir_all(&dir).is_ok() {
                                self.updates.request_reset_search();
                                self.current_dir = dir;
                                self.updates.request_redraw_infobar();
                            }
                            self.updates.request_rescan_files();
                        }
                        // C -> Copy
                        (Focus::Files, KeyCode::Char('c')) => {
                            if let Some(e) = self.dir_content.get(self.current_index) {
                                if let DirContentType::Dir { .. } = e.more {
                                    return Ok(AppCmd::CopyTo(e.entry.path()));
                                }
                            }
                        }
                        // R -> Remove
                        (Focus::Files, KeyCode::Char('r')) => {
                            let paths = self
                                .dir_content
                                .iter()
                                .rev()
                                .filter(|e| e.selected)
                                .map(|e| e.entry.path())
                                .collect();
                            tasks::task_del(paths, share);
                        }
                        // T -> Open Terminal
                        (Focus::Files, KeyCode::Char('t')) => 'term: {
                            Command::new(&share.terminal_command)
                                .current_dir(&self.current_dir)
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .spawn();
                        }
                        // E -> Edit
                        (Focus::Files, KeyCode::Char('e')) => {
                            Self::term_reset(share)?;
                            if let Some(entry) = self.dir_content.get(self.current_index) {
                                let entry_path = entry.entry.path();
                                Command::new(&share.editor_command)
                                    .arg(&entry_path)
                                    .current_dir(&self.current_dir)
                                    .status();
                            }
                            self.term_setup(share)?;
                        }
                        // 0-9 -> set scan_files_max_depth
                        (Focus::Files, KeyCode::Char('0')) => {
                            self.scan_files_max_depth = usize::MAX;
                            self.request_rescan_files_then_select_current_again();
                        }
                        (Focus::Files, KeyCode::Char('1')) => {
                            self.scan_files_max_depth = 0;
                            self.request_rescan_files_then_select_current_again();
                        }
                        (Focus::Files, KeyCode::Char('2')) => {
                            self.scan_files_max_depth = 1;
                            self.request_rescan_files_then_select_current_again();
                        }
                        (Focus::Files, KeyCode::Char('3')) => {
                            self.scan_files_max_depth = 2;
                            self.request_rescan_files_then_select_current_again();
                        }
                        (Focus::Files, KeyCode::Char('4')) => {
                            self.scan_files_max_depth = 3;
                            self.request_rescan_files_then_select_current_again();
                        }
                        (Focus::Files, KeyCode::Char('5')) => {
                            self.scan_files_max_depth = 4;
                            self.request_rescan_files_then_select_current_again();
                        }
                        (Focus::Files, KeyCode::Char('6')) => {
                            self.scan_files_max_depth = 5;
                            self.request_rescan_files_then_select_current_again();
                        }
                        (Focus::Files, KeyCode::Char('7')) => {
                            self.scan_files_max_depth = 6;
                            self.request_rescan_files_then_select_current_again();
                        }
                        (Focus::Files, KeyCode::Char('8')) => {
                            self.scan_files_max_depth = 7;
                            self.request_rescan_files_then_select_current_again();
                        }
                        (Focus::Files, KeyCode::Char('9')) => {
                            self.scan_files_max_depth = 8;
                            self.request_rescan_files_then_select_current_again();
                        }
                        // - - - SearchBar - - -
                        // Esc -> Nevermind
                        (Focus::SearchBar, KeyCode::Esc) => {
                            self.focus = Focus::Files;
                            self.search_text.clear();
                            self.search_regex = None;
                            self.updates.request_redraw_searchbar();
                            self.updates.request_move_cursor();
                            if share.live_search {
                                self.updates.request_filter_files();
                            }
                        }
                        // Enter -> Apply
                        (Focus::SearchBar, KeyCode::Enter) => {
                            self.focus = Focus::Files;
                            self.updates.request_move_cursor();
                            if !share.live_search {
                                self.updates.request_filter_files();
                            }
                            self.updates.request_reset_current_index();
                        }
                        (Focus::SearchBar, KeyCode::Char(ch)) => {
                            self.search_text.push(ch);
                            self.search_regex = None;
                            self.updates.request_redraw_searchbar();
                            if share.live_search {
                                self.updates.request_filter_files();
                            }
                        }
                        (Focus::SearchBar, KeyCode::Backspace) => {
                            self.search_text.pop();
                            self.search_regex = None;
                            self.updates.request_redraw_searchbar();
                            if share.live_search {
                                self.updates.request_filter_files();
                            }
                        }
                        _ => {}
                    },
                    Event::Paste(e) => {}
                    Event::Resize(w, h) => {
                        share.size.0 = w;
                        share.size.1 = h;
                        self.updates.request_redraw();
                    }
                }
            }
        }
    }
}
