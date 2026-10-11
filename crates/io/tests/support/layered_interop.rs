use photocraft_color::BlendMode;
use photocraft_doc::{ColorMode, Document, Layer, LayerContent, LayerMask, PixelFormat, Rect, SampleType, Size, Surface};

pub fn document(depth: SampleType) -> Document {
    let mut doc = Document::new("Layer qualification", Size::new(64, 48), ColorMode::Rgb, depth);
    doc.resolution_dpi = 72.0;
    doc.icc_profile = Some(photocraft_cms::Builtin::Srgb.profile().to_bytes());
    let make = |name: &str, rgba: [f32; 4], noise: bool| {
        let mut surface = Surface::new(PixelFormat::new(ColorMode::Rgb, depth, true));
        let pixels: Vec<f32> = (0..64 * 48)
            .flat_map(|i| {
                let mut p = rgba;
                if noise {
                    p[0] = ((i * 37 % 251) as f32) / 251.;
                    p[1] = ((i * 71 % 241) as f32) / 241.;
                }
                p
            })
            .collect();
        surface.write_region(Rect::new(0, 0, 64, 48), &pixels);
        Layer::new(name, LayerContent::Raster(surface))
    };
    doc.layers.push(make("Paper", [0.85, 0.9, 0.95, 1.], false));
    let mut noise = make("Color noise", [0.5, 0.5, 0.6, 0.7], true);
    noise.opacity = 0.6;
    doc.layers.push(noise);
    let mut multiply = make("Multiply", [0.2, 0.7, 0.3, 0.8], false);
    multiply.blend = BlendMode::Multiply;
    multiply.opacity = 0.7;
    let mut mask = LayerMask::hide_all();
    mask.surface.write_region(Rect::new(0, 0, 32, 48), &vec![1.; 32 * 48]);
    multiply.mask = Some(mask);
    let mut screen = make("Screen", [0.6, 0.15, 0.3, 0.5], false);
    screen.blend = BlendMode::Screen;
    screen.opacity = 0.4;
    doc.layers.push(Layer::group("Paint group", vec![multiply, screen]));
    let mut hidden = make("Hidden \u{84dd}", [0., 0., 1., 1.], false);
    hidden.visible = false;
    doc.layers.push(hidden);
    doc
}

pub fn complex_document(depth: SampleType) -> Document {
    use photocraft_doc::{Adjustment, Color, Fill, Path, ShapeLayer, Subpath, TextLayer};
    let mut doc = document(depth);
    let mut text = TextLayer {
        text: "Editable title".into(),
        font_family: "Inter".into(),
        size_pt: 9.0,
        transform: photocraft_geom::Affine::translate(3.0, 14.0),
        ..Default::default()
    };
    photocraft_text::TextEngine::new().render_layer(&mut text, doc.resolution_dpi, doc.pixel_format());
    let path = Path::new(vec![Subpath::polygon(&[(5., 20.), (30., 20.), (30., 35.), (5., 35.)])]);
    let mut shape = ShapeLayer { path, fill: Some(Fill::Solid(Color::rgb(0.8, 0.2, 0.1))), stroke: None, live: None, cache: None, psd_raw: None };
    shape.cache = Some(photocraft_vector::render_shape(&shape, doc.pixel_format(), doc.bounds()));
    let mut vector = Layer::new("Editable vector", LayerContent::Shape(shape));
    vector.effects.items.push(photocraft_doc::Effect::default_drop_shadow());
    let nested = Layer::group("Nested editing group", vec![vector, Layer::new("Editable title", LayerContent::Text(text))]);
    if let Some(children) = doc.layers.get_mut(2).and_then(Layer::children_mut) {
        children.push(nested);
    }
    doc.layers.push(Layer::new("Invert adjustment", LayerContent::Adjustment(Adjustment::Invert)));
    doc
}
