//! One clipboard paste per Ctrl+V gesture, even with delayed text events or Ctrl released first.
#[derive(Default)]
pub struct PasteGesture {
    armed: bool,
    delivered: bool,
    normal_v_down: bool,
}
impl PasteGesture {
    /// None: another shortcut; Some(true): dispatch Ctrl+V; Some(false): consume duplicate.
    pub fn key(
        &mut self,
        pressed: bool,
        command: bool,
        shift: bool,
        repeat: bool,
        has_event: bool,
    ) -> Option<bool> {
        // egui-winit suppresses Ctrl+V key-down even when there is no clipboard text.
        // A V release without a matching ordinary V press is the remaining shortcut trace,
        // including when Ctrl has already been released. Ordinary typing has a matching press.
        let unmatched_release = !pressed && !self.normal_v_down;
        if pressed && !command {
            self.normal_v_down = true;
        }
        if !pressed {
            self.normal_v_down = false;
        }
        if pressed && command && !shift {
            if !self.armed {
                self.delivered = false;
            }
            self.armed = true;
            let fire = !self.delivered && !repeat && !has_event;
            if fire {
                self.delivered = true;
            }
            Some(fire)
        } else if !pressed && (self.armed || command || unmatched_release) {
            let fire = !self.delivered && !shift;
            self.armed = false;
            self.delivered = false;
            Some(fire)
        } else {
            None
        }
    }
    pub fn clipboard(&mut self, shift: bool) -> bool {
        if shift {
            return true;
        }
        self.armed = true;
        if self.delivered {
            false
        } else {
            self.delivered = true;
            true
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bitmap_paste_works_when_ctrl_is_released_first() {
        let mut p = PasteGesture::default();
        assert_eq!(p.key(true, true, false, false, false), Some(true));
        assert_eq!(p.key(false, false, false, false, false), Some(false));
        assert_eq!(p.key(true, true, false, false, false), Some(true));
    }
    #[test]
    fn text_event_and_key_events_dispatch_exactly_once() {
        let mut p = PasteGesture::default();
        assert_eq!(p.key(true, true, false, false, true), Some(false));
        assert!(p.clipboard(false));
        assert_eq!(p.key(false, true, false, false, false), Some(false));
        assert_eq!(p.key(true, true, false, false, false), Some(true));
        assert!(!p.clipboard(false));
        assert_eq!(p.key(true, true, false, true, false), Some(false));
        assert_eq!(p.key(false, false, false, false, false), Some(false));
    }
    #[test]
    fn release_only_fallback_and_shift_shortcut_do_not_block_next_paste() {
        let mut p = PasteGesture::default();
        assert_eq!(p.key(false, true, false, false, false), Some(true));
        assert!(p.clipboard(true));
        assert_eq!(p.key(false, true, true, false, false), Some(false));
        assert!(p.clipboard(false));
        assert_eq!(p.key(false, false, false, false, false), Some(false));
    }
    #[test]
    fn suppressed_bitmap_press_and_ctrl_released_first_are_detected_but_typing_is_not() {
        let mut p = PasteGesture::default();
        assert_eq!(p.key(false, false, false, false, false), Some(true));
        assert_eq!(p.key(true, false, false, false, false), None);
        assert_eq!(p.key(false, false, false, false, false), None);
        assert_eq!(p.key(false, true, false, false, false), Some(true));
    }
}
