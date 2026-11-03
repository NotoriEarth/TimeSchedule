use super::*;
use crate::core::Renderer as _;

fn render(damage: &[Rectangle], scale: f32) -> tiny_skia::Pixmap {
    render_color(
        damage,
        scale,
        Color::WHITE,
        Rectangle::with_size(Size::new(100., 100.)),
    )
}

fn render_color(
    damage: &[Rectangle],
    scale: f32,
    color: Color,
    quad_bounds: Rectangle,
) -> tiny_skia::Pixmap {
    let bounds = Rectangle::with_size(Size::new(100., 100.));
    let mut renderer = Renderer::new(Font::default(), Pixels(14.));
    renderer.reset(bounds);
    renderer.fill_quad(
        renderer::Quad {
            bounds: quad_bounds,
            ..Default::default()
        },
        color,
    );
    let size = (100. * scale) as u32;
    let mut pixels = tiny_skia::Pixmap::new(size, size).unwrap();
    pixels.fill(tiny_skia::Color::BLACK);
    renderer.draw(
        &mut pixels.as_mut(),
        &mut tiny_skia::Mask::new(size, size).unwrap(),
        &Viewport::with_physical_size(Size::new(size, size), scale),
        damage,
        Color::TRANSPARENT,
    );
    pixels
}

#[test]
fn broad_updates_preserve_translucent_content_and_clear_transparent_background() {
    let damage = [
        Rectangle {
            x: 0.,
            y: 10.,
            width: 100.,
            height: 20.,
        },
        Rectangle {
            x: 0.,
            y: 50.,
            width: 100.,
            height: 20.,
        },
    ];
    let full = [Rectangle::with_size(Size::new(100., 100.))];
    let quad = Rectangle {
        x: 20.,
        y: 20.,
        width: 60.,
        height: 60.,
    };
    let color = Color::from_rgba(0.8, 0.4, 0.2, 0.4);
    for scale in [1., 1.25, 1.5, 2.] {
        let actual = render_color(&damage, scale, color, quad);
        let expected = render_color(&full, scale, color, quad);
        assert!(
            actual.data() == expected.data(),
            "alpha differs at scale {scale}"
        );
        assert_eq!(actual.pixel(0, 0).unwrap().alpha(), 0);
        assert_eq!(
            actual
                .pixel((50. * scale) as u32, (50. * scale) as u32)
                .unwrap()
                .alpha(),
            102
        );
    }
}

#[test]
fn broad_fragmented_damage_redraws_once_as_a_complete_frame() {
    let damage = [
        Rectangle {
            x: 0.,
            y: 10.,
            width: 100.,
            height: 20.,
        },
        Rectangle {
            x: 0.,
            y: 50.,
            width: 100.,
            height: 20.,
        },
    ];
    for scale in [1., 1.25, 1.5, 2.] {
        assert!(
            render(&damage, scale).data()
                == render(&[Rectangle::with_size(Size::new(100., 100.))], scale).data(),
            "broad fragmented updates should use one complete redraw at {scale}",
        );
    }
}

#[test]
fn empty_and_small_damage_preserve_untouched_pixels() {
    let empty = render(&[], 1.);
    assert!(empty.pixels().iter().all(|pixel| pixel.red() == 0));
    let damage: Vec<_> = (0..6)
        .map(|n| Rectangle {
            x: 10. + n as f32 * 10.,
            y: 20.,
            width: 2.,
            height: 2.,
        })
        .collect();
    let small = render(&damage, 1.);
    assert_eq!(small.pixel(0, 0).unwrap().red(), 0);
    assert_eq!(small.pixel(10, 20).unwrap().red(), 255);
    let single = render(
        &[Rectangle {
            x: 0.,
            y: 0.,
            width: 100.,
            height: 20.,
        }],
        1.,
    );
    assert_eq!(single.pixel(20, 10).unwrap().red(), 255);
    assert_eq!(single.pixel(20, 90).unwrap().red(), 0);
}

#[test]
fn large_single_region_also_uses_complete_redraw_at_fractional_scale() {
    let bounds = Rectangle::with_size(Size::new(100., 100.));
    let dirty = Rectangle {
        x: 10.,
        y: 12.5,
        width: 80.,
        height: 75.,
    };
    assert!(render(&[dirty], 1.5).data() == render(&[bounds], 1.5).data());
}
