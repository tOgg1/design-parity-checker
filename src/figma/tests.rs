//! Tests for Figma conversion and transform logic.

#[cfg(test)]
mod tests {
    use crate::figma::api_types::{
        FigmaBoundingBox, FigmaColor, FigmaNodeData, FigmaPaintData, FigmaTypeStyle, ImageFormat,
    };
    use crate::figma::client::{FigmaClient, FigmaError};
    use crate::figma::conversion::collect_figma_nodes;
    use crate::figma::transform::{
        compute_letterbox_transform, finalize_figma_image, normalize_figma_snapshot,
    };
    use crate::types::{BoundingBox, FigmaNode, FigmaSnapshot, TypographyStyle};
    use crate::Viewport;
    use image::{DynamicImage, GenericImageView, RgbaImage};
    use tempfile::TempDir;

    #[test]
    fn test_figma_client_missing_token() {
        let result = FigmaClient::new("");
        assert!(matches!(result.unwrap_err(), FigmaError::MissingToken));
    }

    #[test]
    fn test_image_format_as_str() {
        assert_eq!(ImageFormat::Png.as_str(), "png");
        assert_eq!(ImageFormat::Jpg.as_str(), "jpg");
        assert_eq!(ImageFormat::Svg.as_str(), "svg");
    }

    #[test]
    fn test_figma_color_to_hex() {
        let color = FigmaColor {
            r: 1.0,
            g: 0.5,
            b: 0.0,
            a: 1.0,
        };
        assert_eq!(color.to_hex(), "#ff7f00");
    }

    #[test]
    fn test_figma_color_to_hex_black() {
        let color = FigmaColor {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        assert_eq!(color.to_hex(), "#000000");
    }

    #[tokio::test]
    async fn download_image_propagates_request_error() {
        let client = FigmaClient::new("token").expect("client");

        let result = client
            .download_image("http://127.0.0.1:1/nonexistent")
            .await;

        assert!(
            matches!(result, Err(FigmaError::Request(_))),
            "expected request error, got {:?}",
            result
        );
    }

    #[test]
    fn collect_figma_nodes_maps_typography_and_fills() {
        let child = FigmaNodeData {
            id: "2".to_string(),
            name: "Heading".to_string(),
            node_type: "TEXT".to_string(),
            children: vec![],
            absolute_bounding_box: Some(FigmaBoundingBox {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 30.0,
            }),
            characters: Some("Hello".to_string()),
            style: Some(FigmaTypeStyle {
                font_family: Some("Inter".to_string()),
                font_size: Some(16.0),
                font_weight: Some(600.0),
                line_height_px: Some(24.0),
            }),
            fills: vec![FigmaPaintData {
                paint_type: "SOLID".to_string(),
                color: Some(FigmaColor {
                    r: 0.062745,
                    g: 0.12549,
                    b: 0.188235,
                    a: 1.0,
                }),
                opacity: Some(0.8),
            }],
        };

        let root = FigmaNodeData {
            id: "1".to_string(),
            name: "Frame".to_string(),
            node_type: "FRAME".to_string(),
            children: vec![child],
            absolute_bounding_box: Some(FigmaBoundingBox {
                x: 0.0,
                y: 0.0,
                width: 1200.0,
                height: 800.0,
            }),
            characters: None,
            style: None,
            fills: vec![],
        };

        let mut nodes = Vec::new();
        collect_figma_nodes(&root, &mut nodes);

        let text_node = nodes.iter().find(|n| n.id == "2").expect("text node");
        assert_eq!(text_node.text.as_deref(), Some("Hello"));
        let typo = text_node.typography.as_ref().expect("typography");
        assert_eq!(typo.font_family.as_deref(), Some("Inter"));
        assert_eq!(typo.font_size, Some(16.0));
        assert_eq!(typo.font_weight.as_deref(), Some("600"));
        assert_eq!(typo.line_height, Some(24.0));
        assert_eq!(text_node.fills.len(), 1);
        assert_eq!(text_node.fills[0].color.as_deref(), Some("#0f1f2f"));
        assert_eq!(text_node.fills[0].opacity, Some(0.8));

        let root_node = nodes.iter().find(|n| n.id == "1").expect("root node");
        assert_eq!(root_node.children, vec!["2"]);
        assert!((root_node.bounding_box.width - 1200.0).abs() < f32::EPSILON);
    }

    #[test]
    fn finalize_figma_image_resizes_to_viewport() {
        let dir = TempDir::new().expect("tempdir");
        let out_path = dir.path().join("out.png");

        let img = RgbaImage::from_pixel(10, 5, image::Rgba([255, 0, 0, 255]));
        let img = DynamicImage::ImageRgba8(img);

        let (w, h, _) = finalize_figma_image(
            img,
            &out_path,
            Some(Viewport {
                width: 20,
                height: 20,
            }),
        )
        .expect("finalize");

        assert_eq!((w, h), (20, 20));
        let saved = image::open(&out_path).expect("open saved");
        assert_eq!(saved.dimensions(), (20, 20));
    }

    #[test]
    fn finalize_figma_image_keeps_original_size_when_no_viewport() {
        let dir = TempDir::new().expect("tempdir");
        let out_path = dir.path().join("out.png");

        let img = RgbaImage::from_pixel(12, 8, image::Rgba([0, 0, 255, 255]));
        let img = DynamicImage::ImageRgba8(img);

        let (w, h, transform) = finalize_figma_image(img, &out_path, None).expect("finalize");

        assert_eq!((w, h), (12, 8));
        let saved = image::open(&out_path).expect("open saved");
        assert_eq!(saved.dimensions(), (12, 8));
        assert_eq!(transform.scale, 1.0);
        assert_eq!(transform.offset_x, 0.0);
        assert_eq!(transform.offset_y, 0.0);
    }

    #[test]
    fn normalize_figma_snapshot_offsets_and_scales_to_viewport() {
        let root_bb = BoundingBox {
            x: 100.0,
            y: 200.0,
            width: 100.0,
            height: 50.0,
        };
        let mut snapshot = FigmaSnapshot {
            file_key: "FILE".into(),
            node_id: "root".into(),
            name: Some("Frame".into()),
            nodes: vec![
                FigmaNode {
                    id: "root".into(),
                    name: Some("Frame".into()),
                    node_type: "FRAME".into(),
                    bounding_box: root_bb,
                    text: None,
                    typography: None,
                    fills: vec![],
                    children: vec!["child".into()],
                },
                FigmaNode {
                    id: "child".into(),
                    name: Some("Text".into()),
                    node_type: "TEXT".into(),
                    bounding_box: BoundingBox {
                        x: 120.0,
                        y: 210.0,
                        width: 20.0,
                        height: 10.0,
                    },
                    text: Some("Hi".into()),
                    typography: Some(TypographyStyle {
                        font_family: Some("Inter".into()),
                        font_size: Some(16.0),
                        font_weight: Some("600".into()),
                        line_height: Some(24.0),
                    }),
                    fills: vec![],
                    children: vec![],
                },
            ],
        };

        let letterbox = compute_letterbox_transform(100, 50, 200, 200);
        snapshot = normalize_figma_snapshot(snapshot, Some(root_bb), (100, 50), &letterbox);

        let root = snapshot.nodes.iter().find(|n| n.id == "root").unwrap();
        assert_eq!(root.bounding_box.x, 0.0);
        assert_eq!(root.bounding_box.y, 50.0);
        assert_eq!(root.bounding_box.width, 200.0);
        assert_eq!(root.bounding_box.height, 100.0);

        let child = snapshot.nodes.iter().find(|n| n.id == "child").unwrap();
        assert!((child.bounding_box.x - 40.0).abs() < f32::EPSILON);
        assert!((child.bounding_box.y - 70.0).abs() < f32::EPSILON);
        assert!((child.bounding_box.width - 40.0).abs() < f32::EPSILON);
        assert!((child.bounding_box.height - 20.0).abs() < f32::EPSILON);
    }
}
