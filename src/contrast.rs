//! APCA contrast algorithm — ensures text readability on any background.
//!
//! Copied and adapted from Zed `ui/src/utils/apca_contrast.rs`.

use gpui::Hsla;

/// Minimum APCA contrast ratio for terminal text readability.
///
/// Lc 50 is reasonable for terminal text — terminal themes often have
/// lower contrast than typical UI elements.
pub const MINIMUM_CONTRAST: f32 = 50.0;

struct ApcaConstants {
    main_trc: f32,
    s_rco: f32,
    s_gco: f32,
    s_bco: f32,
    norm_bg: f32,
    norm_txt: f32,
    rev_txt: f32,
    rev_bg: f32,
    blk_thrs: f32,
    blk_clmp: f32,
    scale_bow: f32,
    scale_wob: f32,
    lo_bow_offset: f32,
    lo_wob_offset: f32,
    delta_y_min: f32,
    lo_clip: f32,
}

static APCA_CONSTANTS: ApcaConstants = ApcaConstants {
    main_trc: 2.4,
    s_rco: 0.2126729,
    s_gco: 0.7151522,
    s_bco: 0.0721750,
    norm_bg: 0.56,
    norm_txt: 0.57,
    rev_txt: 0.62,
    rev_bg: 0.65,
    blk_thrs: 0.022,
    blk_clmp: 1.414,
    scale_bow: 1.14,
    scale_wob: 1.14,
    lo_bow_offset: 0.027,
    lo_wob_offset: 0.027,
    delta_y_min: 0.0005,
    lo_clip: 0.1,
};

fn srgb_to_y(color: Hsla, constants: &ApcaConstants) -> f32 {
    let rgba = color.to_rgb();
    let r_linear = (rgba.r).powf(constants.main_trc);
    let g_linear = (rgba.g).powf(constants.main_trc);
    let b_linear = (rgba.b).powf(constants.main_trc);
    constants.s_rco * r_linear + constants.s_gco * g_linear + constants.s_bco * b_linear
}

/// Compute the APCA contrast ratio between two colors.
///
/// Positive values indicate light text on dark background;
/// negative values indicate dark text on light background.
/// An absolute value of 0 means no contrast; typical minimums range
/// from 45 (body text) to 75 (fine text).
pub fn apca_contrast(text_color: Hsla, background_color: Hsla) -> f32 {
    let text_y = srgb_to_y(text_color, &APCA_CONSTANTS);
    let bg_y = srgb_to_y(background_color, &APCA_CONSTANTS);

    let text_y_clamped = if text_y > APCA_CONSTANTS.blk_thrs {
        text_y
    } else {
        text_y + (APCA_CONSTANTS.blk_thrs - text_y).powf(APCA_CONSTANTS.blk_clmp)
    };
    let bg_y_clamped = if bg_y > APCA_CONSTANTS.blk_thrs {
        bg_y
    } else {
        bg_y + (APCA_CONSTANTS.blk_thrs - bg_y).powf(APCA_CONSTANTS.blk_clmp)
    };

    if (bg_y_clamped - text_y_clamped).abs() < APCA_CONSTANTS.delta_y_min {
        return 0.0;
    }

    let output_contrast = if bg_y_clamped > text_y_clamped {
        let sapc = (bg_y_clamped.powf(APCA_CONSTANTS.norm_bg)
            - text_y_clamped.powf(APCA_CONSTANTS.norm_txt))
            * APCA_CONSTANTS.scale_bow;
        if sapc < APCA_CONSTANTS.lo_clip {
            0.0
        } else {
            sapc - APCA_CONSTANTS.lo_bow_offset
        }
    } else {
        let sapc = (bg_y_clamped.powf(APCA_CONSTANTS.rev_bg)
            - text_y_clamped.powf(APCA_CONSTANTS.rev_txt))
            * APCA_CONSTANTS.scale_wob;
        if sapc > -APCA_CONSTANTS.lo_clip {
            0.0
        } else {
            sapc + APCA_CONSTANTS.lo_wob_offset
        }
    };

    output_contrast * 100.0
}

fn adjust_lightness_for_contrast(
    foreground: Hsla,
    background: Hsla,
    minimum_apca_contrast: f32,
) -> Hsla {
    let bg_luminance = srgb_to_y(background, &APCA_CONSTANTS);
    let should_go_darker = bg_luminance > 0.5;

    let mut low = if should_go_darker { 0.0 } else { foreground.l };
    let mut high = if should_go_darker { foreground.l } else { 1.0 };
    let mut best_l = foreground.l;

    for _ in 0..20 {
        let mid = (low + high) / 2.0;
        let test_color = Hsla {
            h: foreground.h,
            s: foreground.s,
            l: mid,
            a: foreground.a,
        };
        let contrast = apca_contrast(test_color, background).abs();
        if contrast >= minimum_apca_contrast {
            best_l = mid;
            if should_go_darker {
                low = mid;
            } else {
                high = mid;
            }
        } else if should_go_darker {
            high = mid;
        } else {
            low = mid;
        }
        if (contrast - minimum_apca_contrast).abs() < 1.0 {
            best_l = mid;
            break;
        }
    }

    Hsla {
        h: foreground.h,
        s: foreground.s,
        l: best_l,
        a: foreground.a,
    }
}

fn adjust_lightness_and_saturation_for_contrast(
    foreground: Hsla,
    background: Hsla,
    minimum_apca_contrast: f32,
) -> Hsla {
    let saturation_steps = [1.0, 0.8, 0.6, 0.4, 0.2, 0.0];
    for &sat_multiplier in &saturation_steps {
        let test_color = Hsla {
            h: foreground.h,
            s: foreground.s * sat_multiplier,
            l: foreground.l,
            a: foreground.a,
        };
        let adjusted = adjust_lightness_for_contrast(test_color, background, minimum_apca_contrast);
        let contrast = apca_contrast(adjusted, background).abs();
        if contrast >= minimum_apca_contrast {
            return adjusted;
        }
    }
    Hsla {
        h: foreground.h,
        s: 0.0,
        l: foreground.l,
        a: foreground.a,
    }
}

/// Ensure a foreground color meets a minimum APCA contrast ratio against
/// its background, adjusting lightness and saturation as needed.
pub fn ensure_minimum_contrast(
    foreground: Hsla,
    background: Hsla,
    minimum_apca_contrast: f32,
) -> Hsla {
    if minimum_apca_contrast <= 0.0 {
        return foreground;
    }
    let current_contrast = apca_contrast(foreground, background).abs();
    if current_contrast >= minimum_apca_contrast {
        return foreground;
    }

    let adjusted = adjust_lightness_for_contrast(foreground, background, minimum_apca_contrast);
    let adjusted_contrast = apca_contrast(adjusted, background).abs();
    if adjusted_contrast >= minimum_apca_contrast {
        return adjusted;
    }

    let desaturated =
        adjust_lightness_and_saturation_for_contrast(foreground, background, minimum_apca_contrast);
    let desaturated_contrast = apca_contrast(desaturated, background).abs();
    if desaturated_contrast >= minimum_apca_contrast {
        return desaturated;
    }

    let black = Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.0,
        a: foreground.a,
    };
    let white = Hsla {
        h: 0.0,
        s: 0.0,
        l: 1.0,
        a: foreground.a,
    };
    let black_contrast = apca_contrast(black, background).abs();
    let white_contrast = apca_contrast(white, background).abs();
    if white_contrast > black_contrast {
        white
    } else {
        black
    }
}
