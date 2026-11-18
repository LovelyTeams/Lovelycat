use std::{fs, io, path::Path};

use gpui::Window;
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{CreatePolygonRgn, CreateRoundRectRgn, HRGN, WINDING},
};

pub const CAT_EAR_HEIGHT: f32 = 40.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    Light,
    Dark,
    Mint,
    Midnight,
}

impl Appearance {
    pub fn toggle(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
            Self::Mint => Self::Midnight,
            Self::Midnight => Self::Mint,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outline {
    #[default]
    Cat,
    Cut,
    Rounded,
    Standard,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Preferences {
    pub appearance: Appearance,
    pub outline: Outline,
}

impl Preferences {
    pub fn load(data: &Path) -> Self {
        fs::read(data.join("appearance.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(self, data: &Path) -> io::Result<()> {
        fs::create_dir_all(data)?;
        let mut file = tempfile::NamedTempFile::new_in(data)?;
        serde_json::to_writer(&mut file, &self)?;
        file.persist(data.join("appearance.json"))?;
        Ok(())
    }
}

#[derive(Default)]
pub struct WindowOutline {
    #[cfg(windows)]
    applied: Option<(Outline, i32, i32, i32)>,
}

impl WindowOutline {
    #[cfg(windows)]
    pub fn apply(&mut self, window: &Window, outline: Outline) -> io::Result<()> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::{
            Foundation::RECT,
            Graphics::Gdi::{DeleteObject, SetWindowRgn},
            UI::WindowsAndMessaging::GetWindowRect,
        };

        let handle = HasWindowHandle::window_handle(window)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Ok(());
        };
        let hwnd = handle.hwnd.get() as _;
        let outline = if window.is_maximized() || window.is_fullscreen() {
            Outline::Standard
        } else {
            outline
        };
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width < 100 || height < 100 {
            return Ok(());
        }
        let cut = (CAT_EAR_HEIGHT / 2. * window.scale_factor()).round() as i32;
        let geometry = if outline == Outline::Standard {
            (outline, 0, 0, 0)
        } else {
            (outline, width, height, cut)
        };
        if self.applied == Some(geometry) {
            return Ok(());
        }

        // A region clips both painting and the hit target. Coordinates use physical pixels.
        let region = create_region(outline, width, height, cut);
        if outline != Outline::Standard && region.is_null() {
            return Err(io::Error::last_os_error());
        }
        if unsafe { SetWindowRgn(hwnd, region, 1) } == 0 {
            let error = io::Error::last_os_error();
            if !region.is_null() {
                unsafe { DeleteObject(region) };
            }
            return Err(error);
        }
        // Windows owns the region after a successful call.
        self.applied = Some(geometry);
        Ok(())
    }

    #[cfg(not(windows))]
    pub fn apply(&mut self, _: &Window, _: Outline) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
fn create_region(outline: Outline, width: i32, height: i32, cut: i32) -> HRGN {
    match outline {
        Outline::Standard => std::ptr::null_mut(),
        Outline::Cat => {
            use windows_sys::Win32::Graphics::Gdi::{CombineRgn, DeleteObject, OffsetRgn, RGN_OR};
            let points: Vec<_> = cat_contour(width as f32, height as f32, cut as f32)
                .into_iter()
                .map(|(x, y)| POINT {
                    x: x.round() as i32,
                    y: y.round() as i32,
                })
                .collect();
            // GDI quantizes to whole pixels. Pad its hit region around all four diagonals
            // so it cannot cut off the DirectComposition surface's multisampled edges.
            unsafe {
                let region = CreatePolygonRgn(points.as_ptr(), points.len() as i32, WINDING);
                let spare = CreatePolygonRgn(points.as_ptr(), points.len() as i32, WINDING);
                if !region.is_null() && !spare.is_null() {
                    for (dx, dy) in [(-2, -2), (4, 0), (0, 4), (-4, 0)] {
                        OffsetRgn(spare, dx, dy);
                        CombineRgn(region, region, spare, RGN_OR);
                    }
                }
                if !spare.is_null() {
                    DeleteObject(spare);
                }
                region
            }
        }
        Outline::Rounded => unsafe {
            CreateRoundRectRgn(0, 0, width + 1, height + 1, cut * 2, cut * 2)
        },
        Outline::Cut => {
            let points = [
                POINT { x: 0, y: 0 },
                POINT {
                    x: width - cut,
                    y: 0,
                },
                POINT { x: width, y: cut },
                POINT {
                    x: width,
                    y: height,
                },
                POINT { x: cut, y: height },
                POINT {
                    x: 0,
                    y: height - cut,
                },
            ];
            unsafe { CreatePolygonRgn(points.as_ptr(), points.len() as i32, WINDING) }
        }
    }
}

/// A shared logical contour for the rendered surface and the native mouse region.
pub fn cat_contour(width: f32, height: f32, unit: f32) -> Vec<(f32, f32)> {
    type Point = (f32, f32);
    fn curve(path: &mut Vec<Point>, a: Point, b: Point, end: Point) {
        let start = *path.last().unwrap();
        for i in 1..=16 {
            let t = i as f32 / 16.;
            let s = 1. - t;
            let at = |p, q, r, e| {
                s * s * s * p + 3. * s * s * t * q + 3. * s * t * t * r + t * t * t * e
            };
            path.push((at(start.0, a.0, b.0, end.0), at(start.1, a.1, b.1, end.1)));
        }
    }
    let u = unit.min(width / 15.).min(height / 8.).max(1.);
    let mut ear = vec![(0., 3. * u)];
    curve(
        &mut ear,
        (0., 2.45 * u),
        (0.62 * u, 0.75 * u),
        (1.04 * u, 0.26 * u),
    );
    curve(
        &mut ear,
        (1.21 * u, 0.04 * u),
        (1.46 * u, 0.03 * u),
        (1.8 * u, 0.19 * u),
    );
    curve(
        &mut ear,
        (3. * u, 0.7 * u),
        (5.25 * u, 2. * u),
        (6. * u, 2. * u),
    );
    let mut cheek = vec![(0., height - 3. * u)];
    curve(
        &mut cheek,
        (0., height - 1.5 * u),
        (1.7 * u, height),
        (4. * u, height),
    );
    ear.iter()
        .copied()
        .chain(ear.iter().rev().map(|&(x, y)| (width - x, y)))
        .chain(cheek.iter().map(|&(x, y)| (width - x, y)))
        .chain(cheek.iter().rev().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_settings_survive_replacing_the_previous_choice() {
        let data = tempfile::tempdir().unwrap();
        let first = Preferences {
            appearance: Appearance::Dark,
            outline: Outline::Rounded,
        };
        first.save(data.path()).unwrap();
        assert_eq!(Preferences::load(data.path()), first);
        let second = Preferences {
            appearance: Appearance::Light,
            outline: Outline::Standard,
        };
        second.save(data.path()).unwrap();
        assert_eq!(Preferences::load(data.path()), second);
    }

    #[test]
    #[cfg(windows)]
    fn region_clips_the_requested_corners() {
        use windows_sys::Win32::Graphics::Gdi::{DeleteObject, PtInRegion};

        let cut = create_region(Outline::Cut, 1000, 700, 20);
        let round = create_region(Outline::Rounded, 1000, 700, 20);
        assert!(!cut.is_null() && !round.is_null());
        let corners = [(1, 1), (998, 1), (998, 698), (1, 698)];
        let cut_hits = corners.map(|(x, y)| unsafe { PtInRegion(cut, x, y) != 0 });
        let round_hits = corners.map(|(x, y)| unsafe { PtInRegion(round, x, y) != 0 });
        unsafe {
            DeleteObject(cut);
            DeleteObject(round);
        }
        assert_eq!(cut_hits, [true, false, true, false]);
        assert_eq!(round_hits, [false; 4]);
        assert!(create_region(Outline::Standard, 1000, 700, 20).is_null());
    }

    #[test]
    #[cfg(windows)]
    fn cat_shape_has_two_ears_and_preserves_the_workbench() {
        use windows_sys::Win32::Graphics::Gdi::{DeleteObject, PtInRegion};

        let region = create_region(Outline::Cat, 1200, 820, 20);
        assert!(!region.is_null());
        let points = [(30, 8), (1170, 8), (600, 8), (600, 44), (24, 790), (1, 819)];
        let hits = points.map(|(x, y)| unsafe { PtInRegion(region, x, y) != 0 });
        unsafe { DeleteObject(region) };
        assert_eq!(hits, [true, true, false, true, true, false]);
    }

    #[test]
    #[cfg(windows)]
    fn antialiased_contour_fits_the_hit_region_at_common_display_scales() {
        use windows_sys::Win32::Graphics::Gdi::{DeleteObject, PtInRegion};
        for scale in [1., 1.25, 1.5, 2.] {
            let (w, h, u) = (1200. * scale, 800. * scale, 20. * scale);
            let region = create_region(Outline::Cat, w as i32, h as i32, u as i32);
            let outline = cat_contour(w, h, u);
            assert!(
                outline
                    .iter()
                    .any(|(x, y)| x.fract() != 0. && y.fract() != 0.)
            );
            let cut = outline
                .iter()
                .filter(|(x, y)| *x > 2. && *x < w - 2. && *y > 2. && *y < h - 2.)
                .find(|(x, y)| unsafe {
                    PtInRegion(region, x.floor() as i32, y.floor() as i32) == 0
                });
            unsafe { DeleteObject(region) };
            assert!(cut.is_none(), "DPI {scale} clipped {cut:?}");
        }
    }
}
