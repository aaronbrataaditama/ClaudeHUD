//! Reveal / hide / pin logic for the panel (§2.3), as a pure state machine.
//! The platform layer turns Win32 mouse messages into `Event`s and carries out `Action`s.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// First mouse move over the strip.
    StripEnter,
    /// Windows' TME_HOVER: the pointer rested on the strip for 250 ms.
    StripHover,
    StripLeave,
    StripClick,
    PanelEnter,
    PanelLeave,
    PinClick,
    TrayClick,
    /// The 300 ms close timer fired.
    CloseTimer,
    /// Workstation locked or a fullscreen app started.
    Suppress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Show,
    Hide,
    StartCloseTimer,
    CancelCloseTimer,
    /// The panel was seen and is closing: acknowledge any latched crash (§1).
    /// Sent with every Hide except the one caused by Suppress (lock/fullscreen),
    /// so a crash row stays visible for as long as the panel is open.
    Acknowledge,
    PinChanged(bool),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hover {
    pub visible: bool,
    pub pinned: bool,
    pub in_strip: bool,
    pub in_panel: bool,
}

impl Hover {
    pub fn step(&mut self, ev: Event) -> Vec<Action> {
        use Action::*;
        let mut out = Vec::new();
        match ev {
            Event::StripEnter => {
                self.in_strip = true;
                if self.visible {
                    out.push(CancelCloseTimer);
                }
            }
            Event::StripHover => {
                self.in_strip = true;
                if !self.visible {
                    self.visible = true;
                    out.push(Show);
                }
            }
            Event::StripLeave => {
                self.in_strip = false;
                self.close_if_outside(&mut out);
            }
            Event::PanelEnter => {
                self.in_panel = true;
                out.push(CancelCloseTimer);
            }
            Event::PanelLeave => {
                self.in_panel = false;
                self.close_if_outside(&mut out);
            }
            Event::StripClick | Event::TrayClick => {
                if self.visible && self.pinned {
                    self.pinned = false;
                    self.visible = false;
                    out.extend([PinChanged(false), Hide, Acknowledge]);
                } else {
                    if !self.visible {
                        self.visible = true;
                        out.push(Show);
                    }
                    self.pinned = true;
                    out.extend([CancelCloseTimer, PinChanged(true)]);
                }
            }
            Event::PinClick => {
                if self.visible {
                    self.pinned = !self.pinned;
                    out.push(PinChanged(self.pinned));
                    if self.pinned {
                        out.push(CancelCloseTimer);
                    } else {
                        self.close_if_outside(&mut out);
                    }
                }
            }
            Event::CloseTimer => {
                if self.visible && !self.pinned && !self.in_strip && !self.in_panel {
                    self.visible = false;
                    out.extend([Hide, Acknowledge]);
                }
            }
            Event::Suppress => {
                if self.visible {
                    self.visible = false;
                    out.push(Hide);
                }
                if self.pinned {
                    self.pinned = false;
                    out.push(PinChanged(false));
                }
            }
        }
        out
    }

    fn close_if_outside(&self, out: &mut Vec<Action>) {
        if self.visible && !self.pinned && !self.in_strip && !self.in_panel {
            out.push(Action::StartCloseTimer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Action::*;
    use super::Event::*;
    use super::*;

    fn opened() -> Hover {
        let mut h = Hover::default();
        h.step(StripEnter);
        h.step(StripHover);
        h
    }

    #[test]
    fn passing_across_the_strip_opens_nothing() {
        let mut h = Hover::default();
        assert!(h.step(StripEnter).is_empty());
        assert!(h.step(StripLeave).is_empty());
        assert!(!h.visible);
    }

    #[test]
    fn dwell_reveals_and_acknowledges() {
        let mut h = Hover::default();
        h.step(StripEnter);
        assert_eq!(h.step(StripHover), vec![Show]);
        assert!(h.visible && !h.pinned);
    }

    #[test]
    fn leaving_both_closes_after_the_timer() {
        let mut h = opened();
        assert_eq!(h.step(StripLeave), vec![StartCloseTimer]);
        assert_eq!(h.step(CloseTimer), vec![Hide, Acknowledge]);
        assert!(!h.visible);
    }

    #[test]
    fn strip_to_panel_traversal_keeps_it_open() {
        let mut h = opened();
        assert_eq!(h.step(StripLeave), vec![StartCloseTimer]);
        assert_eq!(h.step(PanelEnter), vec![CancelCloseTimer]);
        assert!(
            h.step(CloseTimer).is_empty(),
            "late timer is ignored while inside"
        );
        assert!(h.visible);
    }

    #[test]
    fn leave_and_return_within_grace() {
        let mut h = opened();
        h.step(StripLeave);
        h.step(PanelEnter);
        assert_eq!(h.step(PanelLeave), vec![StartCloseTimer]);
        assert_eq!(h.step(StripEnter), vec![CancelCloseTimer]);
        assert!(h.visible);
    }

    #[test]
    fn strip_click_pins_and_second_click_closes() {
        let mut h = Hover::default();
        assert_eq!(
            h.step(StripClick),
            vec![Show, CancelCloseTimer, PinChanged(true)]
        );
        h.step(StripLeave);
        assert!(
            h.step(CloseTimer).is_empty(),
            "pinned ignores the close timer"
        );
        h.step(StripEnter);
        assert_eq!(
            h.step(StripClick),
            vec![PinChanged(false), Hide, Acknowledge]
        );
        assert!(!h.visible && !h.pinned);
    }

    #[test]
    fn pin_glyph_toggles() {
        let mut h = opened();
        h.step(StripLeave);
        h.step(PanelEnter);
        assert_eq!(h.step(PinClick), vec![PinChanged(true), CancelCloseTimer]);
        assert_eq!(
            h.step(PinClick),
            vec![PinChanged(false)],
            "pointer still inside: no close"
        );
        assert_eq!(h.step(PanelLeave), vec![StartCloseTimer]);
    }

    #[test]
    fn tray_click_pins_an_open_panel_and_closes_a_pinned_one() {
        let mut h = opened();
        assert_eq!(h.step(TrayClick), vec![CancelCloseTimer, PinChanged(true)]);
        assert_eq!(
            h.step(TrayClick),
            vec![PinChanged(false), Hide, Acknowledge]
        );
        assert_eq!(
            h.step(TrayClick),
            vec![Show, CancelCloseTimer, PinChanged(true)]
        );
    }

    #[test]
    fn suppress_hides_and_unpins() {
        let mut h = Hover::default();
        h.step(StripClick);
        assert_eq!(h.step(Suppress), vec![Hide, PinChanged(false)]);
        assert!(h.step(Suppress).is_empty());
    }
}
