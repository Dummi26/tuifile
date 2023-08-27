use crossterm::event::{poll, read, Event, KeyCode, KeyModifiers};
use crossterm::style::{Attribute, Color, Stylize};
use crossterm::{cursor, queue, style, terminal, ExecutableCommand};
use regex::RegexBuilder;

use crate::updates::Updates;
use crate::{
    tasks, AppCmd, BackgroundTask, DirContent, DirContentType, Focus, ScanFilesMode, Share,
};
use std::io::Write;
use std::os::unix::prelude::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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
            if let Some(rescan) = share.check_bgtasks() {
                if let Some(task) = &self.dir_content_builder_task {
                    if let Some(v) = {
                        let mut temp = task.lock().unwrap();
                        temp.take()
                    } {
                        self.dir_content_builder_task = None;
                        after_rescanning_files(self, v);
                    }
                }
                if rescan {
                    return Ok(AppCmd::TaskFinished);
                }
            }
            // rescan files if necessary
            fn after_rescanning_files(s: &mut TuiFile, v: Result<Vec<DirContent>, String>) {
                s.updates.request_rescanning_files_complete();
                s.updates.request_filter_files();
                match v {
                    Ok(v) => s.dir_content = v,
                    Err(err) => {
                        s.files_status_is_special = true;
                        s.files_status = err;
                    }
                }
            }
            if self.updates.rescan_files() {
                self.updates.dont_rescan_files();
                if self.dir_content_builder_task.is_none() {
                    self.dir_content.clear();
                    self.files_status_is_special = false;
                    let (scan_dir_blocking, mut scan_dir_threaded, timeout) =
                        match self.scan_files_mode {
                            ScanFilesMode::Blocking => (true, false, None),
                            ScanFilesMode::Threaded => (false, true, None),
                            ScanFilesMode::Timeout(t) => (true, false, Some(t)),
                            ScanFilesMode::TimeoutThenThreaded(t) => (true, true, Some(t)),
                        };
                    if scan_dir_blocking {
                        let v = get_files(
                            self.current_dir.clone(),
                            self.scan_files_max_depth,
                            &share.info_what,
                            timeout,
                        );
                        if v.as_ref().is_ok_and(|v| v.1) {
                            // completed, no need for the threaded fallback.
                            scan_dir_threaded = false;
                        }
                        if !scan_dir_threaded {
                            after_rescanning_files(self, v.map(|v| v.0));
                        }
                    }
                    if scan_dir_threaded {
                        let dir = self.current_dir.clone();
                        let max_depth = self.scan_files_max_depth;
                        let info_what = share.info_what.clone();
                        let arc = Arc::new(Mutex::new(None));
                        self.dir_content_builder_task = Some(Arc::clone(&arc));
                        self.updates.request_redraw_filelist();
                        self.updates.request_redraw_infobar();
                        share.tasks.push(BackgroundTask::new(
                            "listing files...".to_string(),
                            move |_status| {
                                let v = get_files(dir, max_depth, &info_what, None).map(|v| v.0);
                                *arc.lock().unwrap() = Some(v);
                                Ok(())
                            },
                            false,
                        ));
                    }
                    fn get_files(
                        dir: PathBuf,
                        max_depth: usize,
                        info_what: &Vec<u32>,
                        timeout: Option<f32>,
                    ) -> Result<(Vec<DirContent>, bool), String> {
                        let mut o = vec![];
                        let completed = get_files(
                            &mut o,
                            dir,
                            0,
                            max_depth,
                            info_what,
                            timeout.map(|v| (Instant::now(), v)),
                        )?;
                        // table-style
                        let mut lengths = vec![];
                        for e in o.iter() {
                            for (i, line) in e.info.lines().enumerate() {
                                if i >= lengths.len() {
                                    lengths.push(0);
                                }
                                if line.len() > lengths[i] {
                                    lengths[i] = line.len();
                                }
                            }
                        }
                        for e in o.iter_mut() {
                            let src = std::mem::replace(&mut e.info, String::new());
                            for (i, line) in src.lines().enumerate() {
                                let rem = lengths[i] - line.len();
                                if line.starts_with('<') {
                                    e.info.push_str(&line[1..]);
                                    for _ in 0..rem {
                                        e.info.push(' ');
                                    }
                                } else if line.starts_with('>') {
                                    for _ in 0..rem {
                                        e.info.push(' ');
                                    }
                                    e.info.push_str(&line[1..]);
                                } else {
                                    let r = rem / 2;
                                    for _ in 0..r {
                                        e.info.push(' ');
                                    }
                                    e.info.push_str(&line[1..]);
                                    for _ in 0..(rem - r) {
                                        e.info.push(' ');
                                    }
                                }
                            }
                        }
                        fn get_files(
                            dir_content: &mut Vec<DirContent>,
                            dir: PathBuf,
                            depth: usize,
                            max_depth: usize,
                            info_what: &Vec<u32>,
                            time_limit: Option<(Instant, f32)>,
                        ) -> Result<bool, String> {
                            match fs::read_dir(&dir) {
                                Err(e) => {
                                    if depth == 0 {
                                        return Err(format!("{e}"));
                                    }
                                }
                                Ok(files) => {
                                    for entry in files {
                                        if let Ok(entry) = entry {
                                            let mut name =
                                                entry.file_name().to_string_lossy().into_owned();
                                            let metadata = entry.metadata();
                                            let p = entry.path();
                                            let info = if let Ok(metadata) = &metadata {
                                                // in each line:
                                                // first char:
                                                // < left-aligned
                                                // > right-aligned
                                                // anything else -> centered
                                                // sep. line: "< | "
                                                let mut info = String::new();
                                                for info_what in info_what {
                                                    match info_what {
                                                        0 => {
                                                            let mut bytes = metadata.len();
                                                            let mut i = 0;
                                                            loop {
                                                                if bytes < 1024
                                                                    || i + 1 >= BYTE_UNITS.len()
                                                                {
                                                                    info.push_str(&format!(
                                                                        "< | \n>{bytes}\n>{}\n",
                                                                        BYTE_UNITS[i]
                                                                    ));
                                                                    break;
                                                                } else {
                                                                    i += 1;
                                                                    // divide by 1024 but cooler
                                                                    bytes >>= 10;
                                                                }
                                                            }
                                                        }
                                                        1 => {
                                                            info.push_str(&format!(
                                                                "< | \n>{:03o}\n",
                                                                metadata.permissions().mode()
                                                                    & 0o777,
                                                            ));
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                                info
                                            } else {
                                                String::new()
                                            };
                                            let more = match metadata {
                                                Err(e) => DirContentType::Err(e.to_string()),
                                                Ok(metadata) => {
                                                    if metadata.is_symlink() {
                                                        DirContentType::Symlink { metadata }
                                                    } else if metadata.is_file() {
                                                        DirContentType::File { metadata }
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
                                            dir_content.push(DirContent {
                                                path: entry.path(),
                                                name_charlen: name.chars().count(),
                                                name,
                                                rel_depth: depth,
                                                passes_filter: true,
                                                selected: false,
                                                info,
                                                more,
                                            });
                                            if let Some((since, max)) = time_limit {
                                                if since.elapsed().as_secs_f32() > max {
                                                    return Ok(false);
                                                }
                                            }
                                            if depth < max_depth {
                                                get_files(
                                                    dir_content,
                                                    p,
                                                    depth + 1,
                                                    max_depth,
                                                    info_what,
                                                    time_limit,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(true)
                        }
                        Ok((o, completed))
                    }
                }
            }
            if self.updates.rescanning_files_complete() {
                self.updates.dont_rescanning_files_complete();
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
                    self.updates.request_filter_files();
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
                    pathstring.push_str("  -  ");
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
                                    .green()
                                    .underlined()
                                    .bold()
                                    .attribute(Attribute::Underlined)
                            )
                        )?;
                    }
                }
                if self.updates.redraw_filebar() || self.updates.redraw_filelist() {
                    self.updates.request_redraw_filebar();
                    self.updates.dont_redraw_filebar();
                    self.updates.request_move_cursor();
                    self.last_drawn_files_height = share.size.1.saturating_sub(3) as _;
                    let mut status = match self.scan_files_mode {
                        ScanFilesMode::Blocking => " ".to_string(),
                        ScanFilesMode::Threaded => " (t) ".to_string(),
                        ScanFilesMode::Timeout(secs) => format!(" ({secs}s) "),
                        ScanFilesMode::TimeoutThenThreaded(secs) => format!(" ({secs}s -> t) "),
                    };
                    status.push_str(&self.files_status);
                    while status.len() < share.size.0 as usize {
                        status.push(' ');
                    }
                    queue!(
                        share.stdout,
                        cursor::MoveTo(0, 1),
                        style::PrintStyledContent(status.attribute(Attribute::Italic)),
                    )?;
                    if self.updates.redraw_filelist() {
                        self.updates.dont_redraw_filelist();
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
                                    // make text_charlen 1 too large (for the endchar)
                                    text_charlen += 9;
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
                                    while text_charlen < share.size.0 as _ {
                                        text.push(' ');
                                        text_charlen += 1;
                                    }
                                    text.push(endchar);
                                    vec![text.red()]
                                }
                                DirContentType::File { metadata }
                                | DirContentType::Dir { metadata }
                                | DirContentType::Symlink { metadata } => {
                                    let filenamelen =
                                        share.size.0 as usize - 2 - text_charlen - entry.info.len();
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
                                    text.push_str(&entry.info);
                                    text.push(' ');
                                    text.push(endchar);
                                    vec![match entry.more {
                                        DirContentType::File { .. } => text.blue(),
                                        DirContentType::Dir { .. } => text.yellow(),
                                        DirContentType::Symlink { .. } => text.grey(),
                                        DirContentType::Err { .. } => text.red(),
                                    }]
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
                    )?;
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
                        // Ctrl+C/D -> Quit
                        (_, KeyCode::Char('c' | 'd')) if e.modifiers == KeyModifiers::CONTROL => {
                            return Ok(AppCmd::Quit);
                        }
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
                                self.current_dir = entry.path.clone();
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
                        // M -> toggle threaded mode based on searchbar
                        (Focus::Files, KeyCode::Char('m')) => {
                            self.updates.request_reset_search();
                            self.updates.request_redraw_filebar();
                            if self.search_text == "b" {
                                self.scan_files_mode = ScanFilesMode::Blocking;
                            } else if self.search_text == "t" {
                                self.scan_files_mode = ScanFilesMode::Threaded;
                            } else if self.search_text.starts_with("b") {
                                if let Ok(timeout) = self.search_text[1..].parse() {
                                    self.scan_files_mode = ScanFilesMode::Timeout(timeout);
                                }
                            } else if self.search_text.starts_with("t") {
                                if let Ok(timeout) = self.search_text[1..].parse() {
                                    self.scan_files_mode =
                                        ScanFilesMode::TimeoutThenThreaded(timeout);
                                }
                            }
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
                                    return Ok(AppCmd::CopyTo(e.path.clone()));
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
                                .map(|e| e.path.clone())
                                .collect();
                            self.updates.request_redraw_infobar();
                            tasks::task_del(paths, share);
                        }
                        // P -> Permissions
                        (Focus::Files, KeyCode::Char('p')) => {
                            self.updates.request_reset_search();
                            if let Ok(mode) = u32::from_str_radix(&self.search_text, 8) {
                                let paths = self
                                    .dir_content
                                    .iter()
                                    .rev()
                                    .filter(|e| e.selected)
                                    .map(|e| e.path.clone())
                                    .collect();
                                self.updates.request_redraw_infobar();
                                tasks::task_chmod(paths, mode, share);
                            }
                        }
                        // O -> Owner (and group)
                        (Focus::Files, KeyCode::Char('o')) => {
                            self.updates.request_reset_search();
                            // TODO!
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
                                let entry_path = entry.path.clone();
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
                    Event::Paste(_e) => {}
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
