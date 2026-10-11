use super::mixer_live::LiveMixer;
use super::*;
use photocraft_color::PixelFormat;

fn fixture(fmt: PixelFormat) -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"width":192,"height":96,"background":"#808080"})).unwrap();
    s.execute("layer.new.layer", json!({"name":"Source"})).unwrap();
    s.edit("fixture", |doc, id| {
        let (surface, _) = crate::channel_cmds::target_surface(doc, *id, &json!({}))?;
        *surface = Surface::new(fmt);
        surface.fill_rect(Rect::new(0, 0, 96, 96), &[1.0, 0.0, 0.0, 1.0]);
        surface.fill_rect(Rect::new(96, 0, 192, 96), &[0.0, 1.0, 0.0, 1.0]);
        Ok(())
    })
    .unwrap();
    s.execute("layer.new.layer", json!({"name":"Paint"})).unwrap();
    s.edit("depth", |doc, id| {
        *crate::channel_cmds::target_surface(doc, *id, &json!({}))?.0 = Surface::new(fmt);
        Ok(())
    })
    .unwrap();
    s
}

fn target(s: &Session) -> Surface {
    let st = s.active().unwrap();
    st.doc.layer(st.active_layer.unwrap()).unwrap().surface().unwrap().clone()
}

#[test]
fn mixer_live_empty_layer_exact_commit_undo_source_preservation_and_chunking() {
    for fmt in [PixelFormat::RGBA8, PixelFormat::RGBA16, PixelFormat::RGBA32F] {
        let mut s = fixture(fmt);
        assert!(s.tools.brush.mixer.sample_all_layers);
        let before = s.active().unwrap().doc.clone();
        let params = json!({"points":[[40,48]],"seed":47,"size":24,"wet":100,"mix":100,"cleanAfterStroke":false});
        let mut live = LiveMixer::begin(&s, &params).unwrap();
        let id = s.active().unwrap().active_layer.unwrap();
        assert!(live.doc.layer(id).unwrap().surface().unwrap().rgba(40, 48)[3] > 0.0);
        assert_eq!(target(&s).rgba(40, 48)[3], 0.0, "preview must not edit the session");
        let mut points = vec![StrokePoint::new(40.0, 48.0, 1.0)];
        for x in [56.0, 72.0, 88.0, 104.0, 120.0, 136.0, 152.0] {
            let p = StrokePoint::new(x, 48.0, 1.0);
            live.push(&[p]).unwrap();
            points.push(p);
        }
        let preview = live.doc.layer(id).unwrap().surface().unwrap().clone();
        let mut full = params.clone();
        full["points"] = json!(points);
        s.execute("paint.mixerBrush", full).unwrap();
        assert_eq!(preview.read_region(before.bounds()), target(&s).read_region(before.bounds()), "live/replay tolerance is exactly zero");
        for old in &before.layers {
            if old.id != id {
                assert_eq!(
                    old.surface().unwrap().read_region(before.bounds()),
                    s.active().unwrap().doc.layer(old.id).unwrap().surface().unwrap().read_region(before.bounds())
                );
            }
        }
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(target(&s).read_region(before.bounds()), before.layer(id).unwrap().surface().unwrap().read_region(before.bounds()));
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(target(&s).read_region(before.bounds()), preview.read_region(before.bounds()));
    }
}

#[test]
fn mixer_live_current_layer_matches_reference_and_cancels() {
    let mut s = fixture(PixelFormat::RGBA32F);
    let p = json!({"points":[[30,30],[60,34],[100,60]],"seed":9,"size":16,"sampleAllLayers":false,"wet":65,"mix":70,"load":90});
    let mut expected = target(&s);
    let brush = resolve_brush(&s, &p, "paint.mixerBrush").unwrap();
    let mut state = photocraft_paint::mixer::MixerState::default();
    state.load(s.tools.foreground);
    photocraft_paint::mixer::apply_mixer_stroke(
        &mut expected,
        None,
        &Stroke { brush, points: parse_points(&p, "paint.mixerBrush").unwrap() },
        &MixerSettings { wet: 0.65, mix: 0.70, load: 0.90, sample_all_layers: false, ..Default::default() },
        &mut state,
        None,
        false,
    );
    let live = LiveMixer::begin(&s, &p).unwrap();
    let id = s.active().unwrap().active_layer.unwrap();
    let actual = live.doc.layer(id).unwrap().surface().unwrap().read_region(s.active().unwrap().doc.bounds());
    let expected = expected.read_region(s.active().unwrap().doc.bounds());
    let max = actual.iter().zip(expected).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(max <= 1e-6, "reference error {max}");
    drop(live);
    assert_eq!(target(&s).rgba(30, 30)[3], 0.0);
    s.execute("brush.mixer.clean", json!({})).unwrap();
    s.execute("paint.mixerBrush", json!({"points":[[30,30],[60,30]],"sampleAllLayers":false,"wet":0})).unwrap();
    assert_eq!(target(&s).rgba(30, 30)[3], 0.0, "clean brush must stay empty");
}

#[test]
fn mixer_sample_is_spatial_and_never_edits_document() {
    let mut s = fixture(PixelFormat::RGBA8);
    let before = s.active().unwrap().doc.clone();
    s.execute("brush.mixer.sample", json!({"points":[[96,48]],"size":40,"brush":{"hardness":1.0,"pressureSize":false}})).unwrap();
    assert!(std::sync::Arc::ptr_eq(&before, &s.active().unwrap().doc));
    let load = s.tools.mixer.spatial.as_ref().unwrap();
    assert_eq!(load.len(), 4096);
    assert!(load.iter().any(|p| p[0] > 0.9 && p[1] < 0.1 && p[3] > 0.9));
    assert!(load.iter().any(|p| p[1] > 0.9 && p[0] < 0.1 && p[3] > 0.9));
    s.execute("paint.mixerBrush", json!({"points":[[48,48]],"size":40,"wet":0,"mix":0,"brush":{"hardness":1.0,"pressureSize":false}})).unwrap();
    assert!(target(&s).rgba(40, 48)[0] > 0.9);
    assert!(target(&s).rgba(56, 48)[1] > 0.9);
    assert!(s.tools.mixer.spatial.is_some(), "sample persists across strokes");
}

#[test]
fn mixer_live_smoothing_tail_and_invalid_input() {
    let mut s = fixture(PixelFormat::RGBA16);
    let p = json!({"points":[[20,40]],"size":12,"seed":42,"brush":{"smoothing":{"amount":0.6,"catchUpOnEnd":true}}});
    let mut live = LiveMixer::begin(&s, &p).unwrap();
    live.push(&[StrokePoint::new(60.0, 40.0, 0.6)]).unwrap();
    live.push(&[StrokePoint::new(100.0, 48.0, 1.0)]).unwrap();
    let before = live.doc.clone();
    assert!(live.push(&[StrokePoint::new(f64::NAN, 0.0, 1.0)]).is_err());
    assert!(std::sync::Arc::ptr_eq(&before, &live.doc));
    let mut commit = p.clone();
    commit["points"] = json!([[20, 40, 1], [60, 40, 0.6], [100, 48, 1]]);
    s.execute("paint.mixerBrush", commit).unwrap();
    let id = s.active().unwrap().active_layer.unwrap();
    assert_eq!(
        live.doc.layer(id).unwrap().surface().unwrap().read_region(s.active().unwrap().doc.bounds()),
        target(&s).read_region(s.active().unwrap().doc.bounds())
    );
    assert!(s.execute("paint.mixerBrush", json!({"points":[[0,0]],"size":5000})).is_err());
    assert!(s.execute("brush.mixer.sample", json!({"points":[[0,0],[1,1]]})).is_err());
}

#[test]
fn mixer_sampling_obeys_masks_opacity_blend_and_current_layer_switch() {
    let mut s = fixture(PixelFormat::RGBA8);
    let source = s.active().unwrap().doc.layers.iter().find(|l| l.name == "Source").unwrap().id;
    s.edit("source composition", |doc, _| {
        let layer = doc.layer_mut(source).unwrap();
        layer.opacity = 0.4;
        layer.blend = photocraft_color::BlendMode::Multiply;
        layer.mask = Some(photocraft_doc::LayerMask::hide_all());
        Ok(())
    })
    .unwrap();
    let expected = photocraft_compose::render(&s.active().unwrap().doc, Rect::new(39, 39, 40, 40)).px[0];
    s.execute("brush.mixer.sample", json!({"points":[[40,40]],"size":8})).unwrap();
    let actual = s.tools.mixer.reservoir.unwrap();
    for c in 0..3 {
        assert!((actual[c] - expected[c]).abs() < 1e-5);
    }
    s.execute("brush.mixer.clean", json!({})).unwrap();
    s.execute("paint.mixerBrush", json!({"points":[[40,40],[80,40]],"size":8,"wet":100,"mix":100,"sampleAllLayers":false})).unwrap();
    assert_eq!(target(&s).rgba(50, 40)[3], 0.0, "current empty layer provides no paint");
    s.execute("paint.mixerBrush", json!({"points":[[40,40],[80,40]],"size":8,"wet":100,"mix":100,"sampleAllLayers":true})).unwrap();
    assert!(target(&s).rgba(50, 40)[3] > 0.9, "all layers supplies the visible composite");
}

#[test]
fn mixer_overlapping_dabs_resample_the_evolving_composite() {
    let mut s = fixture(PixelFormat::RGBA32F);
    s.execute("tools.setColors", json!({"foreground":"#0000ff"})).unwrap();
    let params = json!({"points":[[50,48]],"size":20,"seed":1,"wet":100,"mix":50,"cleanAfterStroke":false});
    let mut live = LiveMixer::begin(&s, &params).unwrap();
    live.push(&[StrokePoint::new(54.0, 48.0, 1.0), StrokePoint::new(50.0, 48.0, 1.0)]).unwrap();
    let id = s.active().unwrap().active_layer.unwrap();
    let pixel = live.doc.layer(id).unwrap().surface().unwrap().rgba(50, 48);
    assert!(pixel[2] > 0.5, "deposited blue feeds back into later pickups: {pixel:?}");
}

#[test]
fn mixer_prepared_commit_uses_displayed_pixels_and_rejects_stale_documents() {
    let mut s = fixture(PixelFormat::RGBA8);
    let p = json!({"points":[[20,40]],"size":16,"target":"pixels","seed":55});
    let mut live = LiveMixer::begin(&s, &p).unwrap();
    live.push(&[StrokePoint::new(80.0, 40.0, 0.6)]).unwrap();
    let shown = live.doc.clone();
    let id = s.active().unwrap().active_layer.unwrap();
    let steps = s.active().unwrap().history.past_len();
    let commit = live.prepare_commit(&mut s).unwrap();
    assert!(s.prepared_mixer.is_some());
    s.execute("paint.mixerBrush", commit.clone()).unwrap();
    assert!(s.prepared_mixer.is_none());
    assert_eq!(s.active().unwrap().history.past_len(), steps + 1);
    assert_eq!(target(&s).read_region(shown.bounds()), shown.layer(id).unwrap().surface().unwrap().read_region(shown.bounds()));
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("paint.mixerBrush", commit).unwrap();
    assert_eq!(target(&s).read_region(shown.bounds()), shown.layer(id).unwrap().surface().unwrap().read_region(shown.bounds()));
    let stale = LiveMixer::begin(&s, &p).unwrap();
    s.execute("layer.new.layer", json!({})).unwrap();
    assert!(stale.prepare_commit(&mut s).is_err());
}

#[test]
fn mixer_load_commands_preserve_floating_selection_and_history() {
    let mut s = fixture(PixelFormat::RGBA8);
    s.execute("select.all", json!({})).unwrap();
    s.execute("select.float", json!({"dx":8,"dy":0,"copy":true})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let steps = s.active().unwrap().history.past_len();
    for (command, params) in [("brush.mixer.sample", json!({"points":[[96,48]],"size":24})), ("brush.mixer.load", json!({})), ("brush.mixer.clean", json!({}))]
    {
        s.execute(command, params).unwrap();
        assert!(std::sync::Arc::ptr_eq(&doc, &s.active().unwrap().doc));
        assert_eq!(s.active().unwrap().history.past_len(), steps);
        assert!(crate::float_cmds::floating(s.active().unwrap()).is_some());
    }
}

#[test]
fn mixer_rejects_malformed_load_and_retains_or_reloads_paint() {
    let mut s = fixture(PixelFormat::RGBA8);
    let before = s.active().unwrap().doc.clone();
    for state in [json!({"spatial":[]}), json!({"spatial":[[1,0,0,1]]}), json!({"amount":"bad"}), json!({"reservoir":[1e300,0,0,1]})] {
        assert!(s.execute("paint.mixerBrush", json!({"points":[[40,40]],"mixerState":state})).is_err());
        assert!(std::sync::Arc::ptr_eq(&before, &s.active().unwrap().doc));
    }
    assert!(s.execute("brush.mixer.load", json!({"color":[1e300,0,0,1]})).is_err());
    s.execute("brush.mixer.load", json!({"color":"#ff0000"})).unwrap();
    s.execute("paint.mixerBrush", json!({"points":[[40,40],[80,40]],"size":10,"loadAfterStroke":false,"load":10})).unwrap();
    assert!(s.tools.mixer.amount > 0.0 && s.tools.mixer.amount < 1.0);
    s.execute("paint.mixerBrush", json!({"points":[[40,40],[80,40]],"size":10,"loadAfterStroke":true,"load":10})).unwrap();
    assert_eq!(s.tools.mixer.amount, 1.0);
}

#[test]
fn mixer_large_tip_retains_off_canvas_paint_and_commits_the_live_result() {
    let mut s = fixture(PixelFormat::RGBA8);
    let source = s.active().unwrap().doc.clone();
    let params = json!({"points":[[96,48]],"size":1024,"wet":0,"mix":0,"color":"#1122ee","brush":{"pressureSize":false}});
    let live = LiveMixer::begin(&s, &params).unwrap();
    let id = s.active().unwrap().active_layer.unwrap();
    let shown = live.doc.layer(id).unwrap().surface().unwrap().clone();
    assert!(shown.rgba(-100, 48)[3] > 0.99, "off-canvas layer pixels remain paintable");
    let params = live.prepare_commit(&mut s).unwrap();
    s.execute("paint.mixerBrush", params).unwrap();
    assert_eq!(target(&s), shown);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*s.active().unwrap().doc, *source);
}

#[test]
fn local_pigment_live_commit_replay_undo_and_selection_are_exact() {
    for grid in [16, 32, 64] {
        let mut s = fixture(PixelFormat::RGBA16);
        s.execute("select.rect", json!({"x":24,"y":24,"width":128,"height":40})).unwrap();
        let before = s.active().unwrap().doc.clone();
        let params = json!({"points":[[36,48,0.8]],"brush":{"size":48,"hardness":0.3,"pressureSize":false,"mixer":{"reservoirGrid":grid}},"wet":100,"mix":80,"load":20,"cleanAfterStroke":false,"loadAfterStroke":false});
        s.execute("brush.mixer.load", json!({"color":"#60d020"})).unwrap();
        let mut live = LiveMixer::begin(&s, &params).unwrap();
        let points = [StrokePoint::new(60.0, 48.0, 0.6), StrokePoint::new(108.0, 48.0, 1.0), StrokePoint::new(148.0, 48.0, 0.8)];
        for point in points {
            live.push(&[point]).unwrap();
        }
        let shown = live.doc.clone();
        let recorded = live.prepare_commit(&mut s).unwrap();
        s.execute("paint.mixerBrush", recorded.clone()).unwrap();
        let painted = s.active().unwrap().doc.clone();
        assert!(*shown == *painted, "live pixels match commit");
        assert!(s.tools.mixer.local.as_ref().unwrap().valid());
        assert_eq!(target(&s).rgba(80, 12)[3], 0.0);
        assert_eq!(before.layers[0], painted.layers[0]);
        assert_eq!(before.layers[1], painted.layers[1]);
        let state = s.tools.mixer.clone();
        s.execute("edit.undo", json!({})).unwrap();
        assert!(*s.active().unwrap().doc == *before);
        s.execute("paint.mixerBrush", recorded).unwrap();
        assert!(*s.active().unwrap().doc == *painted);
        assert_eq!(s.tools.mixer, state);
        s.execute("edit.undo", json!({})).unwrap();
        s.execute("edit.redo", json!({})).unwrap();
        assert!(*s.active().unwrap().doc == *painted);
    }
}
#[test]
fn local_pigment_rejects_bad_grids_and_replay_state_atomically() {
    let mut s = fixture(PixelFormat::RGBA8);
    let before = s.active().unwrap().doc.clone();
    for grid in [1, 17, 128, u32::MAX] {
        assert!(s.execute("paint.mixerBrush", json!({"points":[[20,20]],"brush":{"mixer":{"reservoirGrid":grid}}})).is_err());
    }
    for local in [json!({"side":u32::MAX,"cells":[]}), json!({"side":64,"cells":[]}), json!({"side":16,"cells":null})] {
        assert!(s.execute("paint.mixerBrush", json!({"points":[[20,20]],"mixerState":{"local":local}})).is_err());
    }
    assert!(std::sync::Arc::ptr_eq(&before, &s.active().unwrap().doc));
}
#[test]
fn local_pigment_preset_round_trip_and_sampling_replace_old_cells() {
    let mut s = fixture(PixelFormat::RGBA8);
    s.execute("tools.setBrush", json!({"size":48,"pressureSize":false,"mixer":{"reservoirGrid":32}})).unwrap();
    s.execute("brush.presets.save", json!({"name":"Local pigment study"})).unwrap();
    s.execute("tools.setBrush", json!({"mixer":{"reservoirGrid":16}})).unwrap();
    s.execute("tools.setBrush", json!({"preset":"Local pigment study"})).unwrap();
    assert_eq!(s.tools.brush.mixer.reservoir_grid, 32);
    s.execute("paint.mixerBrush", json!({"points":[[64,48],[96,48]],"cleanAfterStroke":false,"loadAfterStroke":false})).unwrap();
    assert!(s.tools.mixer.local.is_some());
    s.execute("brush.mixer.sample", json!({"points":[[64,48]]})).unwrap();
    assert!(s.tools.mixer.local.is_none());
    assert!(s.tools.mixer.spatial.is_some());
}

#[test]
fn local_pigment_carried_cells_replay_without_depending_on_current_tool_load() {
    let mut s = fixture(PixelFormat::RGBA16);
    let params = json!({"points":[[36,48],[88,48]],"brush":{"size":48,"pressureSize":false,"mixer":{"reservoirGrid":32}},"cleanAfterStroke":false,"loadAfterStroke":false});
    s.execute("paint.mixerBrush", params.clone()).unwrap();
    let before = s.active().unwrap().doc.clone();
    let mut params = params;
    params["points"] = json!([[88, 48], [132, 56], [156, 48]]);
    let live = LiveMixer::begin(&s, &params).unwrap();
    let shown = live.doc.clone();
    let recorded = live.prepare_commit(&mut s).unwrap();
    assert_eq!(recorded["mixerState"]["local"]["side"], 32);
    s.execute("paint.mixerBrush", recorded.clone()).unwrap();
    let after = s.active().unwrap().doc.clone();
    let carried = s.tools.mixer.clone();
    assert!(*after == *shown);
    s.execute("edit.undo", json!({})).unwrap();
    assert!(*s.active().unwrap().doc == *before);
    s.execute("brush.mixer.load", json!({"color":"#ff00ff"})).unwrap();
    s.execute("paint.mixerBrush", recorded).unwrap();
    assert!(*s.active().unwrap().doc == *after);
    assert_eq!(s.tools.mixer, carried);
}

#[test]
fn trimmed_live_contact_matches_full_bounds_with_masks_blending_and_selection() {
    use photocraft_color::BlendMode;
    use photocraft_doc::LayerMask;
    for fmt in [PixelFormat::RGBA8, PixelFormat::RGBA16, PixelFormat::RGBA32F] {
        for all in [false, true] {
            let mut s = fixture(fmt);
            s.edit("Masked source fixture", |doc, _| {
                let mut mask = LayerMask::reveal_all();
                mask.surface.fill_rect(Rect::new(0, 0, 192, 96), &[0.65]);
                doc.layers[1].mask = Some(mask);
                doc.layers[1].opacity = 0.7;
                doc.layers[1].blend = BlendMode::Multiply;
                Ok(())
            })
            .unwrap();
            s.execute("select.rect", json!({"x":24,"y":20,"width":140,"height":60})).unwrap();
            let original = s.active().unwrap().doc.clone();
            s.execute("brush.mixer.load", json!({"color":"#ce7b25"})).unwrap();
            let original_load = s.tools.mixer.clone();
            let b = crate::knife_cmds::Settings { width: 48.0, contact: 0.18, angle: 31.0, softness: 0.3, breakup: 0.2, ..Default::default() }.brush().unwrap();
            let p = json!({"points":[[36,38,0.7]],"brush":b,"sampleAllLayers":all,"cleanAfterStroke":false,"loadAfterStroke":false});
            let mut trimmed = LiveMixer::begin(&s, &p).unwrap();
            let mut reference = LiveMixer::begin(&s, &p).unwrap();
            reference.use_full_contact_bounds_for_test();
            let rect = Rect::new(0, 0, 192, 96);
            for point in [StrokePoint::new(62.0, 48.0, 1.0), StrokePoint::new(94.0, 62.0, 0.3), StrokePoint::new(140.0, 42.0, 0.8)] {
                trimmed.push(&[point]).unwrap();
                reference.push(&[point]).unwrap();
                assert_eq!(trimmed.doc.layers[2].surface().unwrap().read_region(rect), reference.doc.layers[2].surface().unwrap().read_region(rect));
                assert_eq!(trimmed.doc.layers[..2], original.layers[..2]);
            }
            let params = trimmed.prepare_commit(&mut s).unwrap();
            s.execute("paint.mixerBrush", params).unwrap();
            let pixels = target(&s).read_region(rect);
            let carried = s.tools.mixer.clone();
            s.execute("edit.undo", json!({})).unwrap();
            s.tools.mixer = original_load;
            let params = reference.prepare_commit(&mut s).unwrap();
            s.execute("paint.mixerBrush", params).unwrap();
            assert_eq!(target(&s).read_region(rect), pixels);
            assert_eq!(s.tools.mixer, carried);
        }
    }
}
