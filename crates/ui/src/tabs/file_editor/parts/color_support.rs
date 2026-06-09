#[derive(Clone, Debug, PartialEq)]
struct ComponentCssColor {
    range: std::ops::Range<usize>,
    color: gpui::Hsla,
    format: ComponentCssColorFormat,
}

#[derive(Clone, Debug, PartialEq)]
enum ComponentCssColorFormat {
    Hex { alpha: bool },
    Rgb { alpha: bool },
    Hsl { alpha: bool },
    Hwb { alpha: bool },
    Oklab { alpha: bool },
    Oklch { alpha: bool },
}

struct ComponentCssFunctionArgs<'a> {
    channels: Vec<&'a str>,
    alpha: Option<&'a str>,
}

#[derive(Clone, Copy)]
struct ComponentCssNumber {
    value: f32,
    percent: bool,
}

fn component_color_language(language: Option<&language::LanguageId>) -> bool {
    language.is_some_and(|language| {
        matches!(
            language.as_str(),
            "css"
                | "scss"
                | "sass"
                | "less"
                | "html"
                | "astro"
                | "svelte"
                | "javascriptreact"
                | "typescriptreact"
        )
    })
}

fn component_css_colors(content: &str) -> Vec<ComponentCssColor> {
    let mut colors = Vec::new();
    let mut index = 0;

    while index < content.len() {
        if let Some(color) = component_css_hex_color_at(content, index)
            .or_else(|| component_css_function_color_at(content, index))
        {
            index = color.range.end;
            colors.push(color);
            continue;
        }

        let Some(ch) = content[index..].chars().next() else {
            break;
        };
        index += ch.len_utf8();
    }

    colors
}

fn component_css_color_for_range(
    content: &str,
    range: &std::ops::Range<usize>,
) -> Option<ComponentCssColor> {
    component_css_colors(content)
        .into_iter()
        .find(|color| color.range == *range)
}

fn component_css_hex_color_at(content: &str, index: usize) -> Option<ComponentCssColor> {
    if !content[index..].starts_with('#') || !component_css_boundary_before(content, index) {
        return None;
    }

    let mut end = index + 1;
    let mut digits = String::new();
    for ch in content[end..].chars() {
        if !ch.is_ascii_hexdigit() || digits.len() == 8 {
            break;
        }
        digits.push(ch);
        end += ch.len_utf8();
    }

    if content[end..]
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_hexdigit())
    {
        return None;
    }

    let alpha = match digits.len() {
        3 | 6 => false,
        4 | 8 => true,
        _ => return None,
    };
    let rgba = component_css_hex_to_rgba(&digits)?;

    Some(ComponentCssColor {
        range: index..end,
        color: rgba.into(),
        format: ComponentCssColorFormat::Hex { alpha },
    })
}

fn component_css_hex_to_rgba(digits: &str) -> Option<gpui::Rgba> {
    fn channel(digits: &str, index: usize, short: bool) -> Option<f32> {
        let value = if short {
            let d = &digits[index..index + 1];
            u8::from_str_radix(&format!("{d}{d}"), 16).ok()?
        } else {
            u8::from_str_radix(&digits[index..index + 2], 16).ok()?
        };
        Some(value as f32 / 255.0)
    }

    let short = matches!(digits.len(), 3 | 4);
    let step = if short { 1 } else { 2 };
    Some(gpui::Rgba {
        r: channel(digits, 0, short)?,
        g: channel(digits, step, short)?,
        b: channel(digits, step * 2, short)?,
        a: if matches!(digits.len(), 4 | 8) {
            channel(digits, step * 3, short)?
        } else {
            1.0
        },
    })
}

fn component_css_function_color_at(content: &str, index: usize) -> Option<ComponentCssColor> {
    if !component_css_boundary_before(content, index) {
        return None;
    }

    for function in ["rgba", "rgb", "hsla", "hsl", "hwb", "oklch", "oklab"] {
        if !component_css_starts_with_ignore_ascii(&content[index..], function) {
            continue;
        }

        let open = index + function.len();
        if !content[open..].starts_with('(') {
            continue;
        }

        let close = component_css_matching_paren(content, open)?;
        let args = &content[open + 1..close];
        let (color, format) = component_css_parse_function(function, args)?;
        return Some(ComponentCssColor {
            range: index..close + 1,
            color,
            format,
        });
    }

    None
}

fn component_css_parse_function(
    function: &str,
    args: &str,
) -> Option<(gpui::Hsla, ComponentCssColorFormat)> {
    let parsed = component_css_parse_function_args(args)?;
    match function {
        "rgb" | "rgba" => component_css_parse_rgb(function == "rgba", parsed),
        "hsl" | "hsla" => component_css_parse_hsl(function == "hsla", parsed),
        "hwb" => component_css_parse_hwb(parsed),
        "oklab" => component_css_parse_oklab(parsed),
        "oklch" => component_css_parse_oklch(parsed),
        _ => None,
    }
}

fn component_css_parse_rgb(
    function_alpha: bool,
    args: ComponentCssFunctionArgs<'_>,
) -> Option<(gpui::Hsla, ComponentCssColorFormat)> {
    if args.channels.len() != 3 {
        return None;
    }

    let alpha = args
        .alpha
        .map(component_css_parse_alpha)
        .unwrap_or(Some(1.0))?;
    let rgba = gpui::Rgba {
        r: component_css_parse_rgb_channel(args.channels[0])?,
        g: component_css_parse_rgb_channel(args.channels[1])?,
        b: component_css_parse_rgb_channel(args.channels[2])?,
        a: alpha,
    };

    Some((
        rgba.into(),
        ComponentCssColorFormat::Rgb {
            alpha: function_alpha || args.alpha.is_some(),
        },
    ))
}

fn component_css_parse_hsl(
    function_alpha: bool,
    args: ComponentCssFunctionArgs<'_>,
) -> Option<(gpui::Hsla, ComponentCssColorFormat)> {
    if args.channels.len() != 3 {
        return None;
    }

    let alpha = args
        .alpha
        .map(component_css_parse_alpha)
        .unwrap_or(Some(1.0))?;
    let color = gpui::hsla(
        component_css_parse_angle(args.channels[0])? / 360.0,
        component_css_parse_unit_interval(args.channels[1])?,
        component_css_parse_unit_interval(args.channels[2])?,
        alpha,
    );

    Some((
        color,
        ComponentCssColorFormat::Hsl {
            alpha: function_alpha || args.alpha.is_some(),
        },
    ))
}

fn component_css_parse_hwb(
    args: ComponentCssFunctionArgs<'_>,
) -> Option<(gpui::Hsla, ComponentCssColorFormat)> {
    if args.channels.len() != 3 {
        return None;
    }

    let hue = component_css_parse_angle(args.channels[0])? / 360.0;
    let white = component_css_parse_unit_interval(args.channels[1])?;
    let black = component_css_parse_unit_interval(args.channels[2])?;
    let alpha = args
        .alpha
        .map(component_css_parse_alpha)
        .unwrap_or(Some(1.0))?;
    let sum = white + black;
    let rgba = if sum >= 1.0 {
        let gray = if sum == 0.0 { 0.0 } else { white / sum };
        gpui::Rgba {
            r: gray,
            g: gray,
            b: gray,
            a: alpha,
        }
    } else {
        let base = gpui::hsla(hue, 1.0, 0.5, alpha).to_rgb();
        let factor = 1.0 - sum;
        gpui::Rgba {
            r: base.r * factor + white,
            g: base.g * factor + white,
            b: base.b * factor + white,
            a: alpha,
        }
    };

    Some((
        rgba.into(),
        ComponentCssColorFormat::Hwb {
            alpha: args.alpha.is_some(),
        },
    ))
}

fn component_css_parse_oklab(
    args: ComponentCssFunctionArgs<'_>,
) -> Option<(gpui::Hsla, ComponentCssColorFormat)> {
    if args.channels.len() != 3 {
        return None;
    }

    let lightness = component_css_parse_ok_lightness(args.channels[0])?;
    let a = component_css_parse_ok_axis(args.channels[1])?;
    let b = component_css_parse_ok_axis(args.channels[2])?;
    let alpha = args
        .alpha
        .map(component_css_parse_alpha)
        .unwrap_or(Some(1.0))?;
    let mut rgba = component_css_oklab_to_rgba(lightness, a, b);
    rgba.a = alpha;

    Some((
        rgba.into(),
        ComponentCssColorFormat::Oklab {
            alpha: args.alpha.is_some(),
        },
    ))
}

fn component_css_parse_oklch(
    args: ComponentCssFunctionArgs<'_>,
) -> Option<(gpui::Hsla, ComponentCssColorFormat)> {
    if args.channels.len() != 3 {
        return None;
    }

    let lightness = component_css_parse_ok_lightness(args.channels[0])?;
    let chroma = component_css_parse_ok_axis(args.channels[1])?.max(0.0);
    let hue = component_css_parse_angle(args.channels[2])?.to_radians();
    let alpha = args
        .alpha
        .map(component_css_parse_alpha)
        .unwrap_or(Some(1.0))?;
    let mut rgba = component_css_oklab_to_rgba(lightness, chroma * hue.cos(), chroma * hue.sin());
    rgba.a = alpha;

    Some((
        rgba.into(),
        ComponentCssColorFormat::Oklch {
            alpha: args.alpha.is_some(),
        },
    ))
}

fn component_css_parse_function_args(args: &str) -> Option<ComponentCssFunctionArgs<'_>> {
    let comma_parts = component_css_split_top_level(args, ',');
    if comma_parts.len() > 1 {
        if comma_parts.len() != 3 && comma_parts.len() != 4 {
            return None;
        }
        let channels = comma_parts[..3]
            .iter()
            .map(|part| part.trim())
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if channels.len() != 3 {
            return None;
        }
        return Some(ComponentCssFunctionArgs {
            channels,
            alpha: comma_parts.get(3).map(|part| part.trim()),
        });
    }

    let slash_parts = component_css_split_top_level(args, '/');
    if slash_parts.len() > 2 {
        return None;
    }

    let channels = slash_parts[0]
        .split_whitespace()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    Some(ComponentCssFunctionArgs {
        channels,
        alpha: slash_parts.get(1).map(|part| part.trim()),
    })
}

fn component_css_split_top_level(input: &str, delimiter: char) -> Vec<&str> {
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut parts = Vec::new();

    for (index, ch) in input.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if ch == delimiter && depth == 0 => {
                parts.push(&input[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    parts.push(&input[start..]);
    parts
}

fn component_css_parse_rgb_channel(token: &str) -> Option<f32> {
    let number = component_css_parse_number(token)?;
    let value = if number.percent {
        number.value / 100.0
    } else {
        number.value / 255.0
    };
    Some(value.clamp(0.0, 1.0))
}

fn component_css_parse_unit_interval(token: &str) -> Option<f32> {
    let number = component_css_parse_number(token)?;
    let value = if number.percent || number.value > 1.0 {
        number.value / 100.0
    } else {
        number.value
    };
    Some(value.clamp(0.0, 1.0))
}

fn component_css_parse_ok_lightness(token: &str) -> Option<f32> {
    let number = component_css_parse_number(token)?;
    let value = if number.percent || number.value > 1.0 {
        number.value / 100.0
    } else {
        number.value
    };
    Some(value.clamp(0.0, 1.0))
}

fn component_css_parse_ok_axis(token: &str) -> Option<f32> {
    let number = component_css_parse_number(token)?;
    Some(if number.percent {
        number.value * 0.004
    } else {
        number.value
    })
}

fn component_css_parse_alpha(token: &str) -> Option<f32> {
    let number = component_css_parse_number(token)?;
    let value = if number.percent {
        number.value / 100.0
    } else {
        number.value
    };
    Some(value.clamp(0.0, 1.0))
}

fn component_css_parse_number(token: &str) -> Option<ComponentCssNumber> {
    let token = token.trim();
    if token.eq_ignore_ascii_case("none") || token.is_empty() {
        return None;
    }

    let (number, percent) = token
        .strip_suffix('%')
        .map(|number| (number, true))
        .unwrap_or((token, false));
    Some(ComponentCssNumber {
        value: number.trim().parse().ok()?,
        percent,
    })
}

fn component_css_parse_angle(token: &str) -> Option<f32> {
    let token = token.trim().to_ascii_lowercase();
    if token == "none" || token.is_empty() {
        return None;
    }

    if let Some(value) = token.strip_suffix("turn") {
        return Some(value.trim().parse::<f32>().ok()? * 360.0);
    }
    if let Some(value) = token.strip_suffix("grad") {
        return Some(value.trim().parse::<f32>().ok()? * 0.9);
    }
    if let Some(value) = token.strip_suffix("rad") {
        return Some(value.trim().parse::<f32>().ok()? * 180.0 / std::f32::consts::PI);
    }
    if let Some(value) = token.strip_suffix("deg") {
        return value.trim().parse::<f32>().ok();
    }

    token.parse::<f32>().ok()
}

fn component_css_format_color(format: &ComponentCssColorFormat, color: gpui::Hsla) -> String {
    match format {
        ComponentCssColorFormat::Hex { alpha } => component_css_format_hex(color, *alpha),
        ComponentCssColorFormat::Rgb { alpha } => component_css_format_rgb(color, *alpha),
        ComponentCssColorFormat::Hsl { alpha } => component_css_format_hsl(color, *alpha),
        ComponentCssColorFormat::Hwb { alpha } => component_css_format_hwb(color, *alpha),
        ComponentCssColorFormat::Oklab { alpha } => component_css_format_oklab(color, *alpha),
        ComponentCssColorFormat::Oklch { alpha } => component_css_format_oklch(color, *alpha),
    }
}

fn component_css_format_hex(color: gpui::Hsla, original_alpha: bool) -> String {
    let rgba = color.to_rgb();
    let r = component_css_byte(rgba.r);
    let g = component_css_byte(rgba.g);
    let b = component_css_byte(rgba.b);
    let a = component_css_byte(color.a);
    if original_alpha || color.a < 0.999 {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}")
    }
}

fn component_css_format_rgb(color: gpui::Hsla, original_alpha: bool) -> String {
    let rgba = color.to_rgb();
    let r = component_css_byte(rgba.r);
    let g = component_css_byte(rgba.g);
    let b = component_css_byte(rgba.b);
    if original_alpha || color.a < 0.999 {
        format!(
            "rgba({r}, {g}, {b}, {})",
            component_css_format_alpha(color.a)
        )
    } else {
        format!("rgb({r}, {g}, {b})")
    }
}

fn component_css_format_hsl(color: gpui::Hsla, original_alpha: bool) -> String {
    let h = component_css_format_number((color.h * 360.0).rem_euclid(360.0), 1);
    let s = component_css_format_number(color.s * 100.0, 1);
    let l = component_css_format_number(color.l * 100.0, 1);
    if original_alpha || color.a < 0.999 {
        format!(
            "hsla({h}, {s}%, {l}%, {})",
            component_css_format_alpha(color.a)
        )
    } else {
        format!("hsl({h}, {s}%, {l}%)")
    }
}

fn component_css_format_hwb(color: gpui::Hsla, original_alpha: bool) -> String {
    let rgba = color.to_rgb();
    let h = component_css_format_number((color.h * 360.0).rem_euclid(360.0), 1);
    let w = component_css_format_number(rgba.r.min(rgba.g).min(rgba.b) * 100.0, 1);
    let b = component_css_format_number((1.0 - rgba.r.max(rgba.g).max(rgba.b)) * 100.0, 1);
    if original_alpha || color.a < 0.999 {
        format!(
            "hwb({h} {w}% {b}% / {})",
            component_css_format_alpha(color.a)
        )
    } else {
        format!("hwb({h} {w}% {b}%)")
    }
}

fn component_css_format_oklab(color: gpui::Hsla, original_alpha: bool) -> String {
    let (l, a, b) = component_css_rgba_to_oklab(color.to_rgb());
    let l = component_css_format_number(l * 100.0, 2);
    let a = component_css_format_number(a, 4);
    let b = component_css_format_number(b, 4);
    if original_alpha || color.a < 0.999 {
        format!(
            "oklab({l}% {a} {b} / {})",
            component_css_format_alpha(color.a)
        )
    } else {
        format!("oklab({l}% {a} {b})")
    }
}

fn component_css_format_oklch(color: gpui::Hsla, original_alpha: bool) -> String {
    let (l, a, b) = component_css_rgba_to_oklab(color.to_rgb());
    let chroma = (a * a + b * b).sqrt();
    let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
    let l = component_css_format_number(l * 100.0, 2);
    let chroma = component_css_format_number(chroma, 4);
    let hue = component_css_format_number(hue, 1);
    if original_alpha || color.a < 0.999 {
        format!(
            "oklch({l}% {chroma} {hue} / {})",
            component_css_format_alpha(color.a)
        )
    } else {
        format!("oklch({l}% {chroma} {hue})")
    }
}

fn component_css_format_alpha(alpha: f32) -> String {
    component_css_format_number(alpha.clamp(0.0, 1.0), 3)
}

fn component_css_format_number(value: f32, decimals: usize) -> String {
    let value = if value.abs() < 0.000_5 { 0.0 } else { value };
    let mut text = match decimals {
        0 => format!("{value:.0}"),
        1 => format!("{value:.1}"),
        2 => format!("{value:.2}"),
        3 => format!("{value:.3}"),
        _ => format!("{value:.4}"),
    };
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" { "0".to_string() } else { text }
}

fn component_css_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn component_css_boundary_before(content: &str, index: usize) -> bool {
    content[..index]
        .chars()
        .next_back()
        .is_none_or(|ch| !component_css_ident_char(ch))
}

fn component_css_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

fn component_css_matching_paren(content: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, ch) in content[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn component_css_starts_with_ignore_ascii(content: &str, prefix: &str) -> bool {
    let bytes = content.as_bytes();
    let prefix = prefix.as_bytes();
    bytes.len() >= prefix.len() && bytes[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn component_css_to_linear(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn component_css_from_linear(channel: f32) -> f32 {
    if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

fn component_css_rgba_to_oklab(rgb: gpui::Rgba) -> (f32, f32, f32) {
    let lr = component_css_to_linear(rgb.r);
    let lg = component_css_to_linear(rgb.g);
    let lb = component_css_to_linear(rgb.b);

    let l = 0.412_221_46 * lr + 0.536_332_55 * lg + 0.051_445_995 * lb;
    let m = 0.211_903_5 * lr + 0.680_699_5 * lg + 0.107_396_96 * lb;
    let s = 0.088_302_46 * lr + 0.281_718_85 * lg + 0.629_978_7 * lb;

    let l = l.cbrt();
    let m = m.cbrt();
    let s = s.cbrt();

    (
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    )
}

fn component_css_oklab_to_rgba(l: f32, a: f32, b: f32) -> gpui::Rgba {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    gpui::Rgba {
        r: component_css_from_linear(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s)
            .clamp(0.0, 1.0),
        g: component_css_from_linear(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s)
            .clamp(0.0, 1.0),
        b: component_css_from_linear(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s)
            .clamp(0.0, 1.0),
        a: 1.0,
    }
}
