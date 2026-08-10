use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

use image::{DynamicImage, GenericImageView, RgbaImage};
use log::warn;
use parking_lot::RwLock;

use crate::inventory::Material;
use crate::model;
use crate::render::TextureManager;
use crate::resources;

pub struct ItemRenderer {
    cache: HashMap<String, String>,
    non_bakeable: HashSet<String>,
    texture_manager: Arc<RwLock<TextureManager>>,
    resources: Arc<RwLock<resources::Manager>>,
    model_factory: Arc<RwLock<model::Factory>>,
    texture_images: HashMap<String, DynamicImage>,
}

impl ItemRenderer {
    pub fn new(
        texture_manager: Arc<RwLock<TextureManager>>,
        resources: Arc<RwLock<resources::Manager>>,
        model_factory: Arc<RwLock<model::Factory>>,
    ) -> Self {
        ItemRenderer {
            cache: HashMap::new(),
            non_bakeable: HashSet::new(),
            texture_manager,
            resources,
            model_factory,
            texture_images: HashMap::new(),
        }
    }

    pub fn get_or_bake(&mut self, material: &Material) -> Option<String> {
        let key = Self::material_to_key(material);
        if let Some(name) = self.cache.get(&key) {
            return Some(name.clone());
        }
        if self.non_bakeable.contains(&key) {
            return None;
        }
        let block_name = key.clone();
        match self.bake(material) {
            Some(tex_name) => {
                self.cache.insert(key, tex_name.clone());
                Some(tex_name)
            }
            None => {
                self.non_bakeable.insert(block_name);
                None
            }
        }
    }

    pub fn reset(&mut self) {
        self.cache.clear();
        self.non_bakeable.clear();
        self.texture_images.clear();
    }

    fn material_to_key(material: &Material) -> String {
        let mut result = String::new();
        for (i, c) in format!("{:?}", material).chars().enumerate() {
            if c.is_uppercase() && i != 0 {
                result.push('_');
                result.push(c.to_ascii_lowercase());
            } else {
                result.push(c.to_ascii_lowercase());
            }
        }
        result
    }

    fn bake(&mut self, material: &Material) -> Option<String> {
        let block_name = Self::material_to_key(material);
        let model = {
            let factory = self.model_factory.read();
            match factory.get_item_model("minecraft", &block_name) {
                Some(m) => m,
                None => {
                    warn!("ItemRenderer: no model for {}", block_name);
                    return None;
                }
            }
        };

        self.load_texture_images(&model);
        warn!(
            "ItemRenderer: loaded {} textures for {}",
            self.texture_images.len(),
            block_name
        );

        let img = match self.render_model(&model, Self::tint_for_material(material)) {
            Some(i) => i,
            None => {
                warn!("ItemRenderer: render_model returned None");
                return None;
            }
        };

        if img.as_rgba8().map_or(true, |pixels| pixels.pixels().all(|p| p.0[3] == 0)) {
            warn!("ItemRenderer: baked image for {} is fully transparent", block_name);
            return None;
        }

        let dyn_name = format!("item_3d/{}", &block_name);
        self.texture_manager
            .write()
            .put_dynamic(&dyn_name, img);
        Some(format!("rustcraft-dynamic:{}", dyn_name))
    }

    fn load_texture_images(&mut self, model: &model::Model) {
        for face in &model.faces {
            for tex in &face.vertices_texture {
                if !self.texture_images.contains_key(&tex.name) {
                    if let Some(img) = self.load_texture_image(&tex.name) {
                        self.texture_images.insert(tex.name.clone(), img);
                    }
                }
            }
        }
    }

    fn load_texture_image(&self, name: &str) -> Option<DynamicImage> {
        let (plugin, path) = name.split_once(':')?;
        let path = format!("textures/{}.png", path);
        let mut file = self.resources.read().open(plugin, &path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).ok()?;
        image::load_from_memory(&data).ok()
    }

    fn render_model(
        &self,
        model: &model::Model,
        tint: Option<(u8, u8, u8)>,
    ) -> Option<DynamicImage> {
        const SIZE: u32 = 64;
        let mut output = RgbaImage::new(SIZE, SIZE);
        let mut zbuffer = vec![f64::MAX; (SIZE * SIZE) as usize];

        struct TriData {
            v: [TriVert; 3],
            img: DynamicImage,
            tint: Option<(u8, u8, u8)>,
        }

        let mut tris: Vec<TriData> = Vec::new();

        for face in &model.faces {
            if face.vertices.len() < 4 {
                continue;
            }
            let tex = &face.vertices_texture[0];
            let tex_img = match self.texture_images.get(&tex.name) {
                Some(img) => img.clone(),
                None => continue,
            };

            let face_tint = if face.tint_index == 0 { tint } else { None };
            let mut tv: Vec<TriVert> = Vec::with_capacity(4);
            for vert in &face.vertices {
                let mut x = vert.x as f64 - 0.5;
                let mut y = vert.y as f64 - 0.5;
                let mut z = vert.z as f64 - 0.5;

                // Ry(-45) — spread +X right, +Z left
                let ang_y = (-45.0_f64).to_radians();
                let (sy, cy) = ang_y.sin_cos();
                let tx = x;
                x = tx * cy + z * sy;
                z = -tx * sy + z * cy;

                // Rx(-30) — tilt to see top
                let ang_x = (-30.0_f64).to_radians();
                let (sx, cx) = ang_x.sin_cos();
                let ty = y;
                y = ty * cx - z * sx;
                z = ty * sx + z * cx;

                // Rz(0)
                let ang_z = (0.0_f64).to_radians();
                let (sz, cz) = ang_z.sin_cos();
                let tx = x;
                let ty = y;
                x = tx * cz - ty * sz;
                y = tx * sz + ty * cz;

                // S(0.625) — display.gui scale
                x *= 0.625;
                y *= 0.625;
                z *= 0.625;

                x += 0.5;
                y += 0.5;
                z += 0.5;

                let screen_x = x * (SIZE as f64);
                let screen_y = (1.0 - y) * (SIZE as f64);
                let depth = z;

                let u = vert.toffsetx as f64 / (16.0 * vert.tw as f64);
                let v = vert.toffsety as f64 / (16.0 * vert.th as f64);

                tv.push(TriVert {
                    sx: screen_x,
                    sy: screen_y,
                    depth,
                    u,
                    v,
                });
            }

            tris.push(TriData {
                v: [tv[0], tv[1], tv[2]],
                img: tex_img.clone(),
                tint: face_tint,
            });
            tris.push(TriData {
                v: [tv[0], tv[2], tv[3]],
                img: tex_img.clone(),
                tint: face_tint,
            });
            tris.push(TriData {
                v: [tv[0], tv[1], tv[3]],
                img: tex_img.clone(),
                tint: face_tint,
            });
            tris.push(TriData {
                v: [tv[1], tv[2], tv[3]],
                img: tex_img,
                tint: face_tint,
            });
        }

        for tri in &tris {
            rasterize_triangle(&tri.v, &tri.img, &mut output, &mut zbuffer, tri.tint);
        }

        Some(DynamicImage::ImageRgba8(output))
    }

    fn tint_for_material(material: &Material) -> Option<(u8, u8, u8)> {
        use Material::*;
        match material {
            Grass | GrassBlock | Fern | TallGrass | LargeFern | PottedFern | SugarCane => {
                Some((127, 178, 56))
            }
            OakLeaves | JungleLeaves | AcaciaLeaves | DarkOakLeaves | Vine => {
                Some((106, 173, 75))
            }
            SpruceLeaves => Some((97, 153, 97)),
            BirchLeaves => Some((128, 167, 85)),
            LilyPad => Some((32, 128, 48)),
            Water => Some((63, 118, 228)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct TriVert {
    sx: f64,
    sy: f64,
    depth: f64,
    u: f64,
    v: f64,
}

fn edge_fn(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> f64 {
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

fn epsilon() -> f64 {
    1.0 / 1024.0
}

fn rasterize_triangle(
    v: &[TriVert; 3],
    tex_img: &DynamicImage,
    output: &mut RgbaImage,
    zbuffer: &mut [f64],
    tint: Option<(u8, u8, u8)>,
) {
    let sx0 = v[0].sx.min(v[1].sx.min(v[2].sx)).floor().max(0.0) as u32;
    let sx1 = v[0].sx
        .max(v[1].sx.max(v[2].sx))
        .ceil()
        .min((output.width() - 1) as f64) as u32;
    let sy0 = v[0].sy.min(v[1].sy.min(v[2].sy)).floor().max(0.0) as u32;
    let sy1 = v[0].sy
        .max(v[1].sy.max(v[2].sy))
        .ceil()
        .min((output.height() - 1) as f64) as u32;

    let area = edge_fn(v[0].sx, v[0].sy, v[1].sx, v[1].sy, v[2].sx, v[2].sy);
    if area.abs() < 1e-12 {
        return;
    }

    let (tex_w, tex_h) = tex_img.dimensions();
    let w = output.width();

    for y in sy0..=sy1 {
        for x in sx0..=sx1 {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;

            let w0 = edge_fn(v[1].sx, v[1].sy, v[2].sx, v[2].sy, px, py) / area;
            let w1 = edge_fn(v[2].sx, v[2].sy, v[0].sx, v[0].sy, px, py) / area;
            let w2 = edge_fn(v[0].sx, v[0].sy, v[1].sx, v[1].sy, px, py) / area;

            if w0 < -epsilon() || w1 < -epsilon() || w2 < -epsilon() {
                continue;
            }

            let depth = w0 * v[0].depth + w1 * v[1].depth + w2 * v[2].depth;
            let idx = (y * w + x) as usize;
            if depth > zbuffer[idx] {
                continue;
            }

            let u = w0 * v[0].u + w1 * v[1].u + w2 * v[2].u;
            let v_uv = w0 * v[0].v + w1 * v[1].v + w2 * v[2].v;

            let tx = (u * tex_w as f64).clamp(0.0, (tex_w - 1) as f64) as u32;
            let ty = (v_uv * tex_h as f64).clamp(0.0, (tex_h - 1) as f64) as u32;

            let pixel = tex_img.get_pixel(tx, ty);
            if pixel.0[3] == 0 {
                continue;
            }
            let (pr, pg, pb, pa) = (pixel.0[0], pixel.0[1], pixel.0[2], pixel.0[3]);
            let (cr, cg, cb) = match tint {
                Some((r, g, b)) => (
                    (pr as u32 * r as u32 / 255) as u8,
                    (pg as u32 * g as u32 / 255) as u8,
                    (pb as u32 * b as u32 / 255) as u8,
                ),
                None => (pr, pg, pb),
            };
            output.put_pixel(x, y, image::Rgba([cr, cg, cb, pa]));
            zbuffer[idx] = depth;
        }
    }
}
