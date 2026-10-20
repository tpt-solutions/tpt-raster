//! Surface construction, stride, bounds and borrowed-buffer tests.
use tpt_color::ColorSpaceId;
use tpt_raster::pixel::{Rgba8, Rgba32F};
use tpt_raster::{Surface, SurfaceError};

const RED: Rgba8 = Rgba8 {
    r: 255,
    g: 0,
    b: 0,
    a: 255,
};

#[test]
fn zero_size_surfaces() {
    let s = Surface::<Rgba8>::new(0, 0, ColorSpaceId::Srgb).unwrap();
    assert_eq!(s.as_slice().len(), 0);
    assert_eq!(s.rows().count(), 0);
    assert_eq!(s.try_get_pixel(0, 0), None);

    // Width zero with height non-zero: rows exist but are empty.
    let mut s = Surface::<Rgba8>::new(0, 5, ColorSpaceId::Srgb).unwrap();
    assert_eq!(s.rows().count(), 5);
    assert!(s.rows().all(|r| r.is_empty()));
    assert!(!s.try_set_pixel(0, 0, RED));
}

#[test]
fn owned_surface_is_zeroed_and_addressable() {
    let mut s = Surface::<Rgba8>::new(3, 2, ColorSpaceId::Srgb).unwrap();
    assert_eq!(s.stride(), 3);
    assert_eq!(s.get_pixel(2, 1), Rgba8::default());
    s.set_pixel(2, 1, RED);
    assert_eq!(s.get_pixel(2, 1), RED);
    assert_eq!(s.as_slice()[5], [255, 0, 0, 255]);
}

#[test]
fn edge_pixels_read_and_write() {
    let mut s = Surface::<Rgba8>::new(4, 3, ColorSpaceId::Srgb).unwrap();
    s.set_pixel(0, 0, RED);
    s.set_pixel(3, 2, RED);
    assert_eq!(s.get_pixel(0, 0), RED);
    assert_eq!(s.get_pixel(3, 2), RED);
    assert_eq!(s.get_pixel(3, 0), Rgba8::default());
}

#[test]
fn out_of_bounds_access_is_reported_not_written() {
    let mut s = Surface::<Rgba8>::new(4, 3, ColorSpaceId::Srgb).unwrap();
    assert_eq!(s.try_get_pixel(4, 0), None);
    assert_eq!(s.try_get_pixel(0, 3), None);
    assert!(!s.try_set_pixel(4, 0, RED));
    assert!(!s.try_set_pixel(0, 3, RED));
    assert!(s.as_slice().iter().all(|p| *p == [0, 0, 0, 0]));
}

#[test]
#[should_panic(expected = "outside 4x3")]
fn get_pixel_panics_out_of_bounds() {
    let s = Surface::<Rgba8>::new(4, 3, ColorSpaceId::Srgb).unwrap();
    let _ = s.get_pixel(4, 0);
}

#[test]
#[should_panic(expected = "outside 4x3")]
fn set_pixel_panics_out_of_bounds() {
    let mut s = Surface::<Rgba8>::new(4, 3, ColorSpaceId::Srgb).unwrap();
    s.set_pixel(0, 3, RED);
}

#[test]
fn stride_smaller_than_width_is_rejected() {
    let mut buf = [[0u8; 4]; 16];
    let err = Surface::<Rgba8>::from_slice(&mut buf, 4, 4, 3, ColorSpaceId::Srgb)
        .err()
        .expect("expected an error");
    assert_eq!(
        err,
        SurfaceError::StrideTooSmall {
            stride: 3,
            width: 4
        }
    );
}

#[test]
fn short_buffer_is_rejected() {
    let mut buf = [[0u8; 4]; 9];
    // Needs stride*(h-1)+w = 4*2+4 = 12 pixels.
    let err = Surface::<Rgba8>::from_slice(&mut buf, 4, 3, 4, ColorSpaceId::Srgb)
        .err()
        .expect("expected an error");
    assert_eq!(
        err,
        SurfaceError::BufferTooSmall {
            required: 12,
            actual: 9
        }
    );
}

#[test]
fn last_row_does_not_need_trailing_padding() {
    // Exactly enough for 3 rows of width 4 at stride 6: 6*2 + 4 = 16 pixels.
    let mut buf = [[0u8; 4]; 16];
    assert!(Surface::<Rgba8>::from_slice(&mut buf, 4, 3, 6, ColorSpaceId::Srgb).is_ok());
}

#[test]
fn borrowed_buffer_writes_are_visible_to_the_caller() {
    let mut buf = [[0u8; 4]; 12];
    {
        let mut s =
            Surface::<Rgba8>::from_slice(&mut buf, 3, 2, 6, ColorSpaceId::LinearSrgb).unwrap();
        assert_eq!(s.color_space(), ColorSpaceId::LinearSrgb);
        s.set_pixel(2, 1, RED);
    }
    // Row 1 starts at index 6, so (2, 1) is index 8.
    assert_eq!(buf[8], [255, 0, 0, 255]);
    // Padding (indices 3..6) is never touched.
    assert!(buf[3..6].iter().all(|p| *p == [0, 0, 0, 0]));
}

#[test]
fn padding_is_untouched_by_clear() {
    let mut buf = [[7u8; 4]; 12];
    let mut s = Surface::<Rgba8>::from_slice(&mut buf, 3, 2, 6, ColorSpaceId::Srgb).unwrap();
    s.clear(RED);
    drop(s);
    assert!(buf[0..3].iter().all(|p| *p == [255, 0, 0, 255]));
    assert!(buf[3..6].iter().all(|p| *p == [7, 7, 7, 7]));
    assert!(buf[6..9].iter().all(|p| *p == [255, 0, 0, 255]));
}

#[test]
fn row_iterators_skip_padding() {
    let mut buf = [[0.0f32; 4]; 12];
    let mut s = Surface::<Rgba32F>::from_slice(&mut buf, 3, 2, 6, ColorSpaceId::Srgb).unwrap();
    s.set_pixel(
        0,
        1,
        Rgba32F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        },
    );
    let rows: Vec<usize> = s.rows().map(|r| r.len()).collect();
    assert_eq!(rows, [3, 3]);
    assert_eq!(s.row(1)[0], [1.0; 4]);
}

#[test]
fn zero_stride_with_zero_width_is_valid() {
    let mut buf: [[u8; 4]; 0] = [];
    let s = Surface::<Rgba8>::from_slice(&mut buf, 0, 4, 0, ColorSpaceId::Srgb).unwrap();
    assert_eq!(s.rows().count(), 4);
}
