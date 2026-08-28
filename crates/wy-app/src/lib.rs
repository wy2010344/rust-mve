//! 示例应用与工具：演示 wy-ui 框架的使用。

#[cfg(test)]
mod tests {
    use wy_layout::{FlexChildConvert, FlexObject, Layout, LayoutInsideObject};
    use wy_render::{Color, Point, Primitive, Scene};
    use wy_signal::{GetValue, SetValue, Signal};
    use wy_text::{build_paragraph, FontContext, TextAlign, TextSpan, TextStyle};

    /// 信号 → 布局 → 文本 → 渲染管线集成测试。
    #[test]
    fn full_pipeline_signal_layout_text_scene() {
        // 1. 信号系统
        let count = Signal::new(0);
        assert_eq!(count.get(), 0);
        count.set(5);
        assert_eq!(count.get(), 5);

        // 2. 布局系统
        struct Item {
            idx: usize,
            size: f32,
        }
        impl FlexChildConvert<Item> for TestRow {
            fn index(&self, c: &Item) -> usize {
                c.idx
            }
            fn grow(&self, _c: &Item) -> f32 {
                0.0
            }
            fn outer_size(&self, c: &Item) -> f32 {
                c.size
            }
            fn ignore(&self, _c: &Item) -> bool {
                false
            }
        }
        struct TestRow;
        impl FlexObject<Item> for TestRow {}

        let children = [
            Item {
                idx: 0,
                size: 100.0,
            },
            Item {
                idx: 1,
                size: 200.0,
            },
        ];
        let inside = LayoutInsideObject::new(&children, 500.0);
        let layout = TestRow.to_layout(&inside);
        assert_eq!(layout.child_position(0).unwrap(), 0.0);
        assert_eq!(layout.child_position(1).unwrap(), 100.0);
        assert_eq!(layout.size_from_children().unwrap(), 300.0);

        // 3. 文本排版
        let mut fc = FontContext::new();
        let text = format!("Count: {}", count.get());
        let spans = vec![TextSpan::styled(
            &text,
            TextStyle::normal().with_font_size(24.0),
        )];
        let paragraph = build_paragraph(&mut fc, &spans, Some(500.0), 1, TextAlign::Start).unwrap();
        assert!(paragraph.width() > 0.0);
        assert_eq!(paragraph.text(), "Count: 5");

        // 4. 渲染到 Scene
        let mut scene = Scene::new();
        scene.fill_rect(
            wy_render::Rect::new(0.0, 0.0, 200.0, 100.0),
            Color::from_u32(0xFF_F0F0F0),
        );
        scene.draw_text(
            Point::new(16.0, 16.0),
            &text,
            24.0,
            Color::from_u32(0xFF_000000),
        );
        assert_eq!(scene.len(), 2);

        // 验证图元
        let prims: Vec<_> = scene.iter().collect();
        match prims[0] {
            Primitive::Rect { rect, .. } => {
                assert_eq!(rect.width, 200.0);
                assert_eq!(rect.height, 100.0);
            }
            _ => panic!("expected Rect"),
        }
        match prims[1] {
            Primitive::Text {
                text: t, font_size, ..
            } => {
                assert_eq!(t, "Count: 5");
                assert_eq!(*font_size, 24.0);
            }
            _ => panic!("expected Text"),
        }
    }

    /// 信号变化驱动 Scene 更新。
    #[test]
    fn signal_drives_scene_update() {
        let count = Signal::new(0);
        let mut scene = Scene::new();

        // 初始绘制
        let text = format!("Count: {}", count.get());
        scene.draw_text(Point::new(0.0, 0.0), &text, 16.0, Color::BLACK);
        assert_eq!(scene.len(), 1);

        // 信号变化 → 清空重绘
        count.set(42);
        scene.clear();
        let text = format!("Count: {}", count.get());
        scene.draw_text(Point::new(0.0, 0.0), &text, 16.0, Color::BLACK);
        assert_eq!(scene.len(), 1);

        // 验证文本内容
        let prim = scene.iter().next().unwrap();
        if let Primitive::Text { text: t, .. } = prim {
            assert_eq!(t, "Count: 42");
        } else {
            panic!("expected Text");
        }
    }

    /// 布局 + 文本 + Scene 组合。
    #[test]
    fn layout_text_scene_composition() {
        // 布局计算按钮区域
        struct Btn {
            idx: usize,
            w: f32,
        }
        impl FlexChildConvert<Btn> for BtnRow {
            fn index(&self, c: &Btn) -> usize {
                c.idx
            }
            fn grow(&self, _c: &Btn) -> f32 {
                0.0
            }
            fn outer_size(&self, c: &Btn) -> f32 {
                c.w
            }
            fn ignore(&self, _c: &Btn) -> bool {
                false
            }
        }
        struct BtnRow;
        impl FlexObject<Btn> for BtnRow {}

        let buttons = [Btn { idx: 0, w: 60.0 }, Btn { idx: 1, w: 60.0 }];
        let inside = LayoutInsideObject::new(&buttons, 200.0);
        let layout = BtnRow.to_layout(&inside);

        // 文本排版
        let mut fc = FontContext::new();
        let spans = vec![TextSpan::styled(
            "OK",
            TextStyle::normal().with_font_size(16.0),
        )];
        let paragraph = build_paragraph(&mut fc, &spans, Some(60.0), 1, TextAlign::Center).unwrap();

        // 渲染
        let mut scene = Scene::new();
        for i in 0..2 {
            let x = layout.child_position(i).unwrap();
            let w = layout.child_size(i).unwrap();
            scene.fill_round_rect(
                wy_render::Rect::new(x, 0.0, w, 32.0),
                4.0,
                Color::from_u32(0xFF_D0D0D0),
            );
        }
        // 在第一个按钮上画文字
        let btn_x = layout.child_position(0).unwrap();
        let text_offset = (60.0 - paragraph.width()) / 2.0;
        scene.draw_text(
            Point::new(btn_x + text_offset, 8.0),
            "OK",
            16.0,
            Color::from_u32(0xFF_333333),
        );

        assert_eq!(scene.len(), 3); // 2 rects + 1 text
    }
}
