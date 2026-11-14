//! Conservative AX/Quartz bounds corroboration. Bounds never establish account
//! ownership; that must already be proven independently from each window's PID.
#[derive(Clone, Copy, Debug)]
pub(super) struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Bounds {
    fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width > 0.0
            && self.height > 0.0
    }
    fn matches(self, other: Self) -> bool {
        self.valid()
            && other.valid()
            && [
                self.x - other.x,
                self.y - other.y,
                self.width - other.width,
                self.height - other.height,
            ]
            .iter()
            .all(|v| v.abs() <= 1.0)
    }
}
pub(super) fn corroborated(
    target: Bounds,
    ax: &[Bounds],
    onscreen: &[(u32, Bounds)],
    pid: u32,
) -> bool {
    target.valid()
        && ax.iter().filter(|b| target.matches(**b)).count() == 1
        && onscreen
            .iter()
            .filter(|(owner, b)| *owner == pid && target.matches(*b))
            .count()
            == 1
}
#[cfg(test)]
mod tests {
    use super::*;
    fn bounds() -> Bounds {
        Bounds {
            x: 1.0,
            y: 2.0,
            width: 1200.0,
            height: 800.0,
        }
    }
    #[test]
    fn onscreen_corroboration_requires_correct_pid_and_bounds() {
        let b = bounds();
        assert!(corroborated(b, &[b], &[(42, b)], 42));
        assert!(!corroborated(b, &[b], &[(43, b)], 42));
        assert!(!corroborated(b, &[b], &[], 42));
        assert!(!corroborated(
            b,
            &[b],
            &[(42, Bounds { x: 100.0, ..b })],
            42
        ));
    }
    #[test]
    fn duplicate_ax_or_quartz_geometry_is_indeterminate() {
        let b = bounds();
        assert!(!corroborated(b, &[b, b], &[(42, b)], 42));
        assert!(!corroborated(b, &[b], &[(42, b), (42, b)], 42));
    }
    #[test]
    fn invalid_geometry_cannot_establish_visibility() {
        for value in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            let b = Bounds {
                width: value,
                ..bounds()
            };
            assert!(!corroborated(b, &[b], &[(42, b)], 42));
        }
    }
}
