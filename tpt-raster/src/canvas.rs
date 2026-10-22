//! `tpt-canvas::RasterBackend` for [`Surface`]. Vector work stays in `tpt-canvas`; this
//! module only turns its paths and paints into pixels.

use crate::Surface;
use crate::pixel::PixelFormat;
use crate::rasterize::rasterize_path;
use crate::surface::SurfaceError;
use tpt_canvas::{Affine2, FillRule, FlattenedPath, Paint, RasterBackend};

impl<P: PixelFormat> RasterBackend for Surface<'_, P> {
    type Error = SurfaceError;

    fn fill_path(
        &mut self,
        path: &FlattenedPath,
        transform: &Affine2,
        rule: FillRule,
        paint: &Paint,
    ) -> Result<(), SurfaceError> {
        rasterize_path(self, path, transform, rule, paint)
    }
}
