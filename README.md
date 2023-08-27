# TuiFile

A file explorer for your terminal, with homerow-centric navigation.

TuiFile can

- have multiple instances, one for each open directory
- display recursive directory structures
- filter files using regex
- select multiple files at once
- create new directories
- copy, move and delete
- quickly open your `$TERM` and `$EDITOR`
- build the file list on a background thread to avoid blocking
- add more features (open an issue with ideas if you have any)

## Demo

https://github.com/Dummi26/tuifile/assets/67615357/f2ff5c88-91f2-4161-b16a-603127851f2f

- start in `/tmp/demo/`
- in a new instance (1), navigate to `/markone/Music/`
- select four directories
- go back to instance (0) in `/tmp/demo`
- create a new directory `my music`
- copy the selected directories to this new directory
- enable recursive view
- select some files from subdirectories
- in a third instance, (1) (the previous (1) is now (2)), disable recursive view and go back to `/tmp/demo`
- create a new directory, `fav songs`
- copy the selected files to this new directory
- go back to `my music`
- filter for files containing `.mp3`
- select and then remove all
- clear the filter to reveal remaining files (they are all `.wma` or `.jpg`)
- select all files in `my music` and remove them
- select all files in `/tmp/demo` and remove them
- exit

## Controls

### Global

Ctrl+C/D -> quit
Ctrl+Up/K -> previous
Ctrl+Down/J -> next
Ctrl+Left/H -> close
Ctrl+Right/L -> duplicate

### Normal

- Up/K or Down/J -> move selection
- Left/H -> go to parent directory
- Right/L -> go into directory
- A -> Alternate selection (toggle All)
- S -> Select or toggle current
- D -> Deselect all
- F -> focus Find/Filter bar
- M -> set Mode based on Find/Filter bar (see File List Modes)
- N -> New directory (name taken from find/filter bar text)
- C -> Copy selected to this directory
- R -> remove selected files and directories (not recursive: also requires selecting the directories content)
- P -> set Permissions (mode taken as base-8 number from find/filter bar text)
- O -> set Owner (and group - TODO!)
- 1-9 or 0 -> set recursive depth limit (0 = infinite)
- T -> open terminal here
- E -> open this file in your editor

### Find/Filter Bar

- Esc -> back & discard
- Enter -> back & filter
- Backspace -> delete
- type to enter search regex

## File List Modes

### Blocking

This is the simplest mode. If listing all the files takes a long time, the program will be unresponsive.

To enable, type `b` into the filter bar, go back to files mode, and press `m`.

### Threaded

To avoid blocking, this mode performs all filesystem operations in the background.
Can cause flickering and isn't as responsive on fast disks.

To enable, type `t` into the filter bar, go back to files mode, and press `m`.

### Timeout

Like blocking, but after the timeout is reached, tuifile will stop adding more files to the list.
This means that file lists may be incomplete.

To enable, type `b<seconds>` into the filter bar, go back to files mode, and press `m`.
Replace `<seconds>` with a number like `1` or `0.3`.

### TimeoutThenThreaded

Like blocking, but after the timeout is reached, tuifile will cancel the operation and restart it in threaded mode.

To enable, type `t<seconds>` into the filter bar, go back to files mode, and press `m`.
Replace `<seconds>` with a number like `1` or `0.3`.
