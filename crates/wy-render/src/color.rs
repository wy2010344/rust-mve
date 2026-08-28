//! 颜色类型：RGBA 通道，类型安全，替代散落的 `u32`。

/// 8-bit RGBA 颜色。
///
/// 四个通道取值范围 `[0, 255]`。相比原始 `u32`，使用独立的类型可以在编译期
/// 避免通道顺序错误，并提供 `Color::TRANSPARENT`、`Color::WHITE` 等常用预设。
///
/// 通道顺序按 **sRGBA（红、绿、蓝、Alpha）** 存储，与 GPU 及 Vello 的
/// 期望一致。可通过 [`Color::to_srgb_u32`] 转换为 Vello/CPU 友好的打包整数。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl Color {
    /// 从四个通道值构造颜色（取值范围 `[0, 255]`）。
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// 从 RGB 构造不透明颜色（alpha = 255）。
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// 从 0xRRGGBBAA 的 32 位整数构造颜色。
    pub const fn from_u32(v: u32) -> Self {
        Self {
            r: (v >> 24) as u8,
            g: (v >> 16) as u8,
            b: (v >> 8) as u8,
            a: v as u8,
        }
    }

    /// 打包为 0xRRGGBBAA 的 32 位整数。
    pub const fn to_u32(self) -> u32 {
        ((self.r as u32) << 24) | ((self.g as u32) << 16) | ((self.b as u32) << 8) | self.a as u32
    }

    /// 打包为 Vello/GPU 偏好的 0xRRGGBBAA（直通，与 [`Color::to_u32`] 相同）。
    ///
    /// 该命名用于提示调用方这里产生的是可直接交给后端的打包值。
    pub const fn to_srgb_u32(self) -> u32 {
        self.to_u32()
    }

    /// 提取 alpha 通道（`[0, 255]`）。
    pub const fn alpha(self) -> u8 {
        self.a
    }

    /// 提取红色通道（`[0, 255]`）。
    pub const fn red(self) -> u8 {
        self.r
    }

    /// 提取绿色通道（`[0, 255]`）。
    pub const fn green(self) -> u8 {
        self.g
    }

    /// 提取蓝色通道（`[0, 255]`）。
    pub const fn blue(self) -> u8 {
        self.b
    }

    /// 完全透明（全零）。
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    /// 不透明黑色。
    pub const BLACK: Color = Color::rgb(0, 0, 0);

    /// 不透明白色。
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    /// 不透明浅灰（常用作面板底色）。
    pub const LIGHT_GRAY: Color = Color::rgb(230, 230, 230);

    /// 不透明中灰。
    pub const GRAY: Color = Color::rgb(128, 128, 128);

    /// 不透明深灰。
    pub const DARK_GRAY: Color = Color::rgb(64, 64, 64);

    /// 不透明红色。
    pub const RED: Color = Color::rgb(255, 0, 0);

    /// 不透明绿色。
    pub const GREEN: Color = Color::rgb(0, 128, 0);

    /// 不透明蓝色。
    pub const BLUE: Color = Color::rgb(0, 0, 255);

    /// 不透明青色。
    pub const CYAN: Color = Color::rgb(0, 255, 255);

    /// 不透明品红。
    pub const MAGENTA: Color = Color::rgb(255, 0, 255);

    /// 不透明黄色。
    pub const YELLOW: Color = Color::rgb(255, 255, 0);
}

impl From<u32> for Color {
    fn from(v: u32) -> Self {
        Color::from_u32(v)
    }
}

impl From<Color> for u32 {
    fn from(c: Color) -> Self {
        c.to_u32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_rgba_roundtrip() {
        let c = Color::rgba(12, 34, 56, 78);
        assert_eq!(c.red(), 12);
        assert_eq!(c.green(), 34);
        assert_eq!(c.blue(), 56);
        assert_eq!(c.alpha(), 78);
        assert_eq!(c.to_u32(), 0x0c22384e);
    }

    #[test]
    fn color_from_u32_roundtrip() {
        let v = 0x11223344;
        let c = Color::from_u32(v);
        assert_eq!(c.to_u32(), v);
        assert_eq!(Color::from(c), c);
        assert_eq!(u32::from(c), v);
    }

    #[test]
    fn color_transparent_alpha_is_zero() {
        assert_eq!(Color::TRANSPARENT.alpha(), 0);
        assert_eq!(Color::TRANSPARENT.to_u32(), 0);
    }

    #[test]
    fn color_white_is_opaque() {
        assert_eq!(Color::WHITE.alpha(), 255);
        assert_eq!(Color::WHITE.to_u32(), 0xffffffff);
    }
}
