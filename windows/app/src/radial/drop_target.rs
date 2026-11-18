//! `IDropTarget` of the radial overlay: tracks the hovered segment and turns a drop of files
//! into a `ToEngine::RadialDrop` (PLAN section 5.4).

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use windows::core::{implement, Ref};
use windows::Win32::Foundation::{HWND, LPARAM, POINTL, WPARAM};
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, ReleaseStgMedium, CF_HDROP, DROPEFFECT, DROPEFFECT_COPY,
    DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::geometry::{hit_test, Hit};
use super::{Shared, Target, WM_RADIAL_HOVER};
use crate::bridge::ToEngine;

#[implement(IDropTarget)]
pub struct DropTarget {
    shared: Arc<Shared>,
}

impl DropTarget {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self { shared }
    }

    /// Hover state for a screen point: -2 outside, -1 cancel disc, otherwise the segment.
    fn hover_of(&self, pt: &POINTL) -> i32 {
        let guard = self
            .shared
            .layout
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(l) = guard.as_ref() else { return -2 };
        let dx = f64::from(pt.x - l.centre_screen.0);
        let dy = f64::from(pt.y - l.centre_screen.1);
        match hit_test(dx, dy, l.targets.len(), l.layout.inner, l.layout.outer) {
            Hit::Segment(i) => i32::try_from(i).unwrap_or(-2),
            Hit::Cancel => -1,
            Hit::Outside => -2,
        }
    }

    fn track(&self, pt: &POINTL, effect: *mut DROPEFFECT) {
        let h = self.hover_of(pt);
        // SAFETY: OLE passes a valid pointer to the in/out effect value.
        unsafe {
            if !effect.is_null() {
                *effect = if h >= 0 {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_NONE
                };
            }
        }
        if self.shared.hover.swap(h, Ordering::Relaxed) != h {
            self.notify(WM_RADIAL_HOVER);
        }
    }

    fn notify(&self, msg: u32) {
        let hwnd = self.shared.ui_hwnd.load(Ordering::Relaxed);
        // SAFETY: posting to a window handle from its own thread; failure is harmless.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd as *mut std::ffi::c_void)),
                msg,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

/// Paths in a `CF_HDROP` payload.
fn dropped_paths(data: &IDataObject) -> Vec<PathBuf> {
    let fmt = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    // SAFETY: `fmt` describes a global-memory CF_HDROP; the medium is released below. The HDROP
    // is only used while the medium is alive.
    unsafe {
        let Ok(mut medium) = data.GetData(&fmt) else {
            return Vec::new();
        };
        let hdrop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        let mut out = Vec::new();
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            let got = DragQueryFileW(hdrop, i, Some(&mut buf)) as usize;
            out.push(PathBuf::from(String::from_utf16_lossy(
                &buf[..got.min(len)],
            )));
        }
        ReleaseStgMedium(&mut medium);
        out
    }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        _data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.track(pt, effect);
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.track(pt, effect);
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        if self.shared.hover.swap(-2, Ordering::Relaxed) != -2 {
            self.notify(WM_RADIAL_HOVER);
        }
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let hover = self.hover_of(pt);
        // SAFETY: OLE passes a valid pointer to the in/out effect value.
        unsafe {
            if !effect.is_null() {
                *effect = if hover >= 0 {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_NONE
                };
            }
        }
        if hover >= 0 {
            let target = {
                let guard = self
                    .shared
                    .layout
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard
                    .as_ref()
                    .and_then(|l| l.targets.get(usize::try_from(hover).unwrap_or(usize::MAX)))
                    .cloned()
            };
            let paths = data.as_ref().map(dropped_paths).unwrap_or_default();
            if let (Some(target), false) = (target, paths.is_empty()) {
                let to = match target {
                    Target::All => None,
                    Target::Peer { id, .. } => Some(id),
                };
                let _ = self.shared.tx.send(ToEngine::RadialDrop { paths, to });
            }
        }
        self.shared.hover.store(-2, Ordering::Relaxed);
        self.notify(super::WM_RADIAL_END);
        Ok(())
    }
}
