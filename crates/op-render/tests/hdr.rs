//! Compare GPU delivery codes against independent scalar Rec.2100 reference equations.
use half::f16;
use op_core::{Primaries, Transfer};
use op_media::{DeliveryColor, VideoInput};
use op_render::{Gpu, Renderer, Tex};

fn renderer() -> Option<Renderer> {
    let device = if std::env::var("OPENPREMIER_HDR_SOFTWARE").as_deref() == Ok("1") {
        Gpu::software()
    } else {
        Gpu::headless()
    };
    match device {
        Ok(gpu) => {
            eprintln!("HDR adapter: {gpu:?}");
            Some(Renderer::new(gpu))
        }
        Err(e) if std::env::var("OPENPREMIER_REQUIRE_GPU").as_deref() == Ok("1") => panic!("{e}"),
        Err(e) => {
            eprintln!("HDR pixel tests skipped: {e}");
            None
        }
    }
}
fn upload(r: &mut Renderer, c: [f32; 4]) -> Tex {
    let pixel: Vec<u8> = c
        .into_iter()
        .flat_map(|x| f16::from_f32(x).to_le_bytes())
        .collect();
    r.upload_working(4, 4, &pixel.repeat(16)).unwrap()
}
fn pq(nits: f64) -> f64 {
    let p = (nits / 10000.0).clamp(0.0, 1.0).powf(2610.0 / 16384.0);
    ((3424.0 / 4096.0 + 2413.0 / 128.0 * p) / (1.0 + 2392.0 / 128.0 * p)).powf(2523.0 / 32.0)
}
fn hlg(nits: f64) -> f64 {
    let scene = (nits / 1000.0).powf(1.0 / 1.2);
    if scene <= 1.0 / 12.0 {
        (3.0 * scene).sqrt()
    } else {
        0.17883277 * (12.0 * scene - 0.28466892).ln() + 0.55991073
    }
}
fn code(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

#[test]
fn absolute_luminance_and_transparent_black_deliver_real_ten_bit_hdr() {
    let Some(mut r) = renderer() else { return };
    for (color, values) in [
        (
            DeliveryColor::Pq,
            vec![0.0, 0.1, 1.0, 100.0, 203.0, 1000.0, 4000.0, 10000.0],
        ),
        (
            DeliveryColor::Hlg,
            vec![0.0, 0.1, 1.0, 100.0, 203.0, 1000.0],
        ),
    ] {
        for nits in values {
            let working = (nits / 203.0_f64).powf(1.0 / 2.4) as f32;
            let frame = upload(&mut r, [working, working, working, 1.0]);
            let pending = r.begin_delivery_color(&frame, VideoInput::Yuv420p10, color);
            let planes = r.end_read(pending).unwrap();
            let reference = if color == DeliveryColor::Pq {
                pq(nits)
            } else {
                hlg(nits)
            };
            let expected = (64.0 + 876.0 * reference).round() as i32;
            assert!(
                (i32::from(code(&planes[0])) - expected).abs() <= 2,
                "{color:?} {nits} cd/m²: {} != {expected}",
                code(&planes[0])
            );
            assert!(code(&planes[1]).abs_diff(512) <= 1);
            assert!(code(&planes[2]).abs_diff(512) <= 1);
            r.recycle(frame);
        }
        let transparent = upload(&mut r, [0.0, 0.0, 0.0, 0.0]);
        let pending = r.begin_delivery_color(&transparent, VideoInput::Yuv420p10, color);
        assert_eq!(code(&r.end_read(pending).unwrap()[0]), 64);
        r.recycle(transparent);
    }
}

#[test]
fn pq_and_hlg_wide_gamut_sources_roundtrip_without_sdr_clipping() {
    let Some(mut r) = renderer() else { return };
    for (transfer, color) in [
        (Transfer::Pq, DeliveryColor::Pq),
        (Transfer::Hlg, DeliveryColor::Hlg),
    ] {
        for c in [
            [0.8, 0.3, 0.1, 1.0],
            [0.1, 0.8, 0.3, 1.0],
            [0.3, 0.1, 0.8, 1.0],
            [0.45, 0.3, 0.2, 0.5],
        ] {
            let source = upload(&mut r, c);
            let working = r.input_hdr(&source, transfer, Primaries::Bt2020);
            let pending = r.begin_delivery_color(&working, VideoInput::Yuva444p10, color);
            let planes = r.end_read(pending).unwrap();
            let rgb = c[..3]
                .iter()
                .map(|x| f64::from(*x / c[3]))
                .collect::<Vec<_>>();
            let y = 0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2];
            let reference = [
                64.0 + 876.0 * y,
                512.0 + 896.0 * (rgb[2] - y) / 1.8814,
                512.0 + 896.0 * (rgb[0] - y) / 1.4746,
            ];
            for i in 0..3 {
                assert!(
                    (f64::from(code(&planes[i])) - reference[i]).abs() <= 3.0,
                    "{transfer:?} {c:?} plane {i}: {} vs {}",
                    code(&planes[i]),
                    reference[i]
                );
            }
            assert!(code(&planes[3]).abs_diff((1023.0 * c[3]).round() as u16) <= 1);
            r.recycle(source);
            r.recycle(working);
        }
    }
}

#[test]
fn opaque_hdr_composites_alpha_in_linear_light_and_sdr_defaults_stay_unchanged() {
    let Some(mut r) = renderer() else { return };
    let frame = upload(&mut r, [0.5, 0.5, 0.5, 0.5]);
    let pending = r.begin_delivery_color(&frame, VideoInput::Yuv420p10, DeliveryColor::Pq);
    let expected = (64.0 + 876.0 * pq(101.5)).round() as i32;
    assert!((i32::from(code(&r.end_read(pending).unwrap()[0])) - expected).abs() <= 2);
    let a = r.begin_delivery(&frame, VideoInput::Yuv420p);
    let a = r.end_read(a).unwrap();
    let b = r.begin_delivery_color(&frame, VideoInput::Yuv420p, DeliveryColor::Bt709);
    assert_eq!(a, r.end_read(b).unwrap());
}
