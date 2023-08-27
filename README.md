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

using i3 (wm) and zellij (green border).

https://github.com/Dummi26/tuifile/assets/67615357/0b0553c9-72e5-4d38-8537-f6cc39147ab1

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
- M -> set Mode bysed on Find/Filter bar
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
