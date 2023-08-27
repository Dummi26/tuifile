pub(crate) trait Updates {
    fn request_redraw(&mut self) {
        self.request_redraw_infobar();
        self.request_redraw_searchbar();
        self.request_redraw_filelist();
    }

    fn rescan_files(&self) -> bool;
    fn dont_rescan_files(&mut self);
    fn request_rescan_files(&mut self);

    fn clear(&self) -> bool;
    fn dont_clear(&mut self);
    fn request_clear(&mut self);

    fn redraw_infobar(&self) -> bool;
    fn dont_redraw_infobar(&mut self);
    fn request_redraw_infobar(&mut self);

    fn redraw_searchbar(&self) -> bool;
    fn dont_redraw_searchbar(&mut self);
    fn request_redraw_searchbar(&mut self);

    fn redraw_filelist(&self) -> bool;
    fn dont_redraw_filelist(&mut self);
    fn request_redraw_filelist(&mut self);

    fn move_cursor(&self) -> bool;
    fn dont_move_cursor(&mut self);
    fn request_move_cursor(&mut self);

    fn filter_files(&self) -> bool;
    fn dont_filter_files(&mut self);
    fn request_filter_files(&mut self);

    fn reset_current_index(&self) -> bool;
    fn dont_reset_current_index(&mut self);
    fn request_reset_current_index(&mut self);

    fn reset_search(&self) -> bool;
    fn dont_reset_search(&mut self);
    fn request_reset_search(&mut self);

    fn rescanning_files_complete(&self) -> bool;
    fn dont_rescanning_files_complete(&mut self);
    fn request_rescanning_files_complete(&mut self);

    fn redraw_filebar(&self) -> bool;
    fn dont_redraw_filebar(&mut self);
    fn request_redraw_filebar(&mut self);
}
impl Updates for u32 {
    fn rescan_files(&self) -> bool {
        0 != self & 0b1
    }
    fn dont_rescan_files(&mut self) {
        *self ^= 0b1;
    }
    fn request_rescan_files(&mut self) {
        *self |= 0b1;
    }
    fn clear(&self) -> bool {
        0 != self & 0b10
    }
    fn dont_clear(&mut self) {
        *self ^= 0b10;
    }
    fn request_clear(&mut self) {
        *self |= 0b10;
    }
    fn redraw_infobar(&self) -> bool {
        0 != self & 0b100
    }
    fn dont_redraw_infobar(&mut self) {
        *self ^= 0b100;
    }
    fn request_redraw_infobar(&mut self) {
        *self |= 0b100;
    }
    fn redraw_searchbar(&self) -> bool {
        0 != self & 0b1000
    }
    fn dont_redraw_searchbar(&mut self) {
        *self ^= 0b1000;
    }
    fn request_redraw_searchbar(&mut self) {
        *self |= 0b1000;
    }
    fn redraw_filelist(&self) -> bool {
        0 != self & 0b10000
    }
    fn dont_redraw_filelist(&mut self) {
        *self ^= 0b10000;
    }
    fn request_redraw_filelist(&mut self) {
        *self |= 0b10000;
    }
    fn move_cursor(&self) -> bool {
        0 != self & 0b100000
    }
    fn dont_move_cursor(&mut self) {
        *self ^= 0b100000;
    }
    fn request_move_cursor(&mut self) {
        *self |= 0b100000;
    }
    fn filter_files(&self) -> bool {
        0 != self & 0b1000000
    }
    fn dont_filter_files(&mut self) {
        *self ^= 0b1000000;
    }
    fn request_filter_files(&mut self) {
        *self |= 0b1000000;
    }
    fn reset_current_index(&self) -> bool {
        0 != self & 0b10000000
    }
    fn dont_reset_current_index(&mut self) {
        *self ^= 0b10000000;
    }
    fn request_reset_current_index(&mut self) {
        *self |= 0b10000000;
    }
    fn reset_search(&self) -> bool {
        0 != self & 0b100000000
    }
    fn dont_reset_search(&mut self) {
        *self ^= 0b100000000;
    }
    fn request_reset_search(&mut self) {
        *self |= 0b100000000;
    }
    fn rescanning_files_complete(&self) -> bool {
        0 != self & 0b1000000000
    }
    fn dont_rescanning_files_complete(&mut self) {
        *self ^= 0b1000000000;
    }
    fn request_rescanning_files_complete(&mut self) {
        *self |= 0b1000000000;
    }
    fn redraw_filebar(&self) -> bool {
        0 != self & 0b10000000000
    }
    fn dont_redraw_filebar(&mut self) {
        *self ^= 0b10000000000;
    }
    fn request_redraw_filebar(&mut self) {
        *self |= 0b10000000000;
    }
}
