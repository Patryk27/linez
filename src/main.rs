use bresenham::Bresenham;
use clap::Parser;
use image::{ImageReader, RgbImage};
use minifb::{Key, Window, WindowOptions};
use rand::Rng;
use rand::RngCore;
use std::path::PathBuf;
use std::thread;

#[derive(Parser)]
struct Args {
    target: PathBuf,

    #[clap(short, long, default_value = "1")]
    threads: usize,

    #[clap(short, long, default_value = "4096")]
    iterations: usize,
}

fn main() {
    let args = Args::parse();

    // ---

    let target = ImageReader::open(args.target)
        .expect("couldn't load given image")
        .decode()
        .expect("couldn't decode given image")
        .into_rgb8();

    let target = Image::from(target);
    let width = target.width;
    let height = target.height;

    let approx = Image::from(RgbImage::new(width, height));
    let mut approxes = vec![approx; args.threads];

    // ---

    let mut canvas = vec![0; (width * height) as usize];

    let mut window = Window::new(
        "linez",
        width as usize,
        height as usize,
        WindowOptions::default(),
    )
    .unwrap();

    while window.is_open() && !window.is_key_down(Key::Escape) {
        thread::scope(|s| {
            for approx in &mut approxes {
                s.spawn(|| {
                    let mut rng = rand::thread_rng();

                    for _ in 0..args.iterations {
                        tick(&mut rng, &target, approx);
                    }
                });
            }
        });

        compose(&mut canvas, &target, &approxes);

        window
            .update_with_buffer(&canvas, width as usize, height as usize)
            .unwrap();
    }
}

fn tick(rng: &mut impl RngCore, target: &Image, approx: &mut Image) -> bool {
    // Randomize starting point
    let beg_x = rng.gen_range(0..target.width) as isize;
    let beg_y = rng.gen_range(0..target.height) as isize;

    // Randomize ending point
    let end_x = rng.gen_range(0..target.width) as isize;
    let end_y = rng.gen_range(0..target.height) as isize;

    // Randomize color
    let r = rng.gen_range(0..255);
    let g = rng.gen_range(0..255);
    let b = rng.gen_range(0..255);

    // Prepare changes required to draw the line.
    //
    // We're using a closure, since `Brasenham` is not `Clone`-able and, for
    // performance reasons, we'd like to avoid `.collect()`-ing the temporary
    // points here.
    let width = target.width;
    let height = target.height;

    let changes = || {
        Bresenham::new((beg_x, beg_y), (end_x, end_y))
            .flat_map(|(x, y)| {
                if x >= 0 && x < (width as isize) && y >= 0 && y < (height as isize) {
                    Some([x as u32, y as u32])
                } else {
                    None
                }
            })
            .map(|pos| (pos, [r, g, b]))
    };

    // Check if `approx + changes()` brings us "closer" towards `target`
    let loss_delta = Image::loss_delta(target, approx, changes());

    // ... if not, bail out
    if loss_delta >= 0.0 {
        return false;
    }

    // ... and otherwise apply the changes, i.e. draw the line
    approx.apply(changes());

    true
}

type Point = [u32; 2];
type Color = [u8; 3];

#[derive(Clone)]
struct Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Image {
    /// Calculates how good `approx + changes` approximates `target`, i.e. we
    /// evaluate `Δ distance(target, approx + changes)` in color space.
    ///
    /// Intuitively, if this function returns a *negative* value, it means that
    /// `approx + changes` approximates `target` *better* than just `approx` and
    /// so it makes sense to apply `changes` on `approx`.
    ///
    /// (negative = better, since we expect the loss to converge to zero over
    /// infinite time.)
    ///
    /// Note that we're interested only in the *change* of the loss - we don't
    /// care whether the absolute value of loss is 123.0 or 321.0, just whether
    /// `approx + changes` gets us closer towards `target`, even if only by a
    /// slight margin.
    ///
    /// Calculating absolute loss would require going through all of the pixels
    /// and that'd be wasteful, since we don't care about the precise value of
    /// the loss.
    fn loss_delta(
        target: &Self,
        approx: &Self,
        changes: impl IntoIterator<Item = (Point, Color)>,
    ) -> f32 {
        changes
            .into_iter()
            .map(|(pos, new_col)| {
                let target_color = target.color_at(pos);
                let approx_color = approx.color_at(pos);

                let loss_without_changes = Self::pixel_loss(target_color, approx_color);
                let loss_with_changes = Self::pixel_loss(target_color, new_col);

                loss_with_changes - loss_without_changes
            })
            .sum()
    }

    /// Calculates how far apart `a` is from `b`.
    ///
    /// We use mean squared error, which is basically squared Euclidian distance
    /// between the channels of given RGB colors.
    ///
    /// Note that since RGB is not a perceptual color model¹, calculating loss
    /// this way is not ideal - but it's good enough.
    ///
    /// ¹ distances in RGB space don't correspond to how humans perceive
    ///   distances between colors, e.g. compare with CIELab.
    fn pixel_loss(a: Color, b: Color) -> f32 {
        a.into_iter()
            .zip(b)
            .map(|(a, b)| (a as f32 - b as f32).powi(2))
            .sum()
    }

    fn apply(&mut self, changes: impl IntoIterator<Item = (Point, Color)>) {
        for (pos, col) in changes {
            *self.color_at_mut(pos) = col;
        }
    }

    fn color_at(&self, point: Point) -> Color {
        let offset = (point[1] * self.width + point[0]) as usize * 3;
        let color = &self.pixels[offset..][..3];

        color.try_into().unwrap()
    }

    fn color_at_mut(&mut self, [x, y]: [u32; 2]) -> &mut Color {
        let offset = (y * self.width + x) as usize * 3;
        let color = &mut self.pixels[offset..][..3];

        color.try_into().unwrap()
    }
}

impl From<RgbImage> for Image {
    fn from(img: RgbImage) -> Self {
        let width = img.width();
        let height = img.height();
        let pixels = img.pixels().flat_map(|pixel| pixel.0).collect();

        Self {
            width,
            height,
            pixels,
        }
    }
}

fn compose(canvas: &mut Vec<u32>, target: &Image, images: &[Image]) {
    let mut buf = canvas.iter_mut();

    for y in 0..target.height {
        for x in 0..target.width {
            let target = target.color_at([x, y]);

            let winner = images
                .iter()
                .map(|image| {
                    let color = image.color_at([x, y]);
                    let loss = Image::pixel_loss(color, target);

                    (color, loss)
                })
                .min_by(|(_, a), (_, b)| a.total_cmp(b))
                .unwrap()
                .0;

            let [r, g, b] = winner;

            *buf.next().unwrap() = u32::from_be_bytes([0, r, g, b]);
        }
    }
}
