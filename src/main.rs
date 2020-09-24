//#![windows_subsystem = "windows"]
mod intel8080;
mod machine;

use machine::{Machine, PlayerKey};
use ::image::{RgbaImage, Rgba};
use piston_window::*;

const SCALE: f64 = 3.0;

fn main() -> std::io::Result<()> {
  let mut emulator = Machine::new();
  let mut window: PistonWindow =
    WindowSettings::new("Space Invaders", [224. * SCALE, 256. * SCALE])
      .exit_on_esc(true)
      //.graphics_api(OpenGL::V4_5)
      .graphics_api(OpenGL::V3_2)
      .samples(16)
      .build()
      .unwrap();

  #[cfg(not(feature = "cputest"))]
  {
    emulator.load_rom_bytes(include_bytes!("../roms/invaders.h"));
    emulator.load_rom_bytes_at(include_bytes!("../roms/invaders.g"), 0x800);
    emulator.load_rom_bytes_at(include_bytes!("../roms/invaders.f"), 0x1000);
    emulator.load_rom_bytes_at(include_bytes!("../roms/invaders.e"), 0x1800);
    //emulator.load_rom("roms/invaders.bin")?;
    //emulator.load_rom("roms/invaders.h")?;
    //emulator.load_rom_at("roms/invaders.g", 0x800)?;
    //emulator.load_rom_at("roms/invaders.f", 0x1000)?;
    //emulator.load_rom_at("roms/invaders.e", 0x1800)?;
  }

  #[cfg(feature = "cputest")]
  emulator.load_rom("roms/cputest.bin")?;

  let mut screen = RgbaImage::new(224, 256);
  let mut texture_context = window.create_texture_context();
  let texture_settings = TextureSettings::new();

  let background = ::image::load_from_memory(include_bytes!("../images/background.jpg")).unwrap();
  let background = match background {
    ::image::DynamicImage::ImageRgba8(image) => image,
    image => image.to_rgba(),
  };
  let background = ::image::imageops::resize(&background, 224 * SCALE as u32, 256 * SCALE as u32, ::image::imageops::FilterType::Lanczos3);
  let background = Texture::from_image(
    &mut texture_context,
    &background,
    &texture_settings,
  ).unwrap();
  let mut show_background = false;

  while let Some(event) = window.next() {
    window.draw_2d(&event, |context, graphics, _device| {
      clear([0.0; 4], graphics);

      let screen_buffer = emulator.frame_buffer();
      // In the actual Space Invaders machine, the screen is drawn sideways and the monitor is physically rotated 90 CCW
      // This means the data in memory starts with the bottom left corner
      for y in 0..256 {
        for x in 0..224 {
          let index = (x * 32) + ((255 - y) / 8);
          let byte = screen_buffer[index as usize];
          let bit = byte & (1 << (255 - y) % 8);
          screen.put_pixel(x, y, match bit {
            0 => match y {
              //32..=63 => Rgba([0xFF, 0x00, 0x00, 0xFF]),
              //184..=239 => Rgba([0x00, 0xFF, 0x00, 0xFF]),
              //240..=255 if x > 23 && x < 136 => Rgba([0x00, 0xFF, 0x00, 0xFF]),
              _ => Rgba([0x00, 0x00, 0x00, 0x00]),
            }
            _ => match y {
              32..=63 => Rgba([0xFF, 0x00, 0x00, 0xFF]),
              184..=239 => Rgba([0x00, 0xFF, 0x00, 0xFF]),
              240..=255 if x > 23 && x < 136 => Rgba([0x00, 0xFF, 0x00, 0xFF]),
              _ => Rgba([0xFF, 0xFF, 0xFF, 0xFF]),
            }
          });
        }
      }

      let screen = Texture::from_image(
        &mut texture_context,
        &screen,
        &texture_settings,
      ).unwrap();

      if show_background {
        image(
          &background,
          context.transform,
          graphics,
        );
      }

      image(
        &screen,
        context.transform.scale(SCALE, SCALE),
        graphics,
      );
    });

    if let Some(_args) = event.update_args() {
      //TODO: Use args.dt?
      emulator.execute();
    }

    if let Some(args) = event.button_args() {
      match args.button {
        Button::Keyboard(key) => match key {
          Key::C => match args.state {
            ButtonState::Press => emulator.key_down(PlayerKey::Coin),
            ButtonState::Release => emulator.key_up(PlayerKey::Coin),
          }
          Key::T => match args.state {
              ButtonState::Press => emulator.key_down(PlayerKey::Tilt),
              ButtonState::Release => emulator.key_up(PlayerKey::Tilt),
          }
          Key::D1 => match args.state {
            ButtonState::Press => emulator.key_down(PlayerKey::P1Start),
            ButtonState::Release => emulator.key_up(PlayerKey::P1Start),
          }
          Key::D2 => match args.state {
            ButtonState::Press => emulator.key_down(PlayerKey::P2Start),
            ButtonState::Release => emulator.key_up(PlayerKey::P2Start),
          }
          Key::Space => match args.state {
            ButtonState::Press => emulator.key_down(PlayerKey::P1Fire),
            ButtonState::Release => emulator.key_up(PlayerKey::P1Fire),
          }
          Key::Slash => match args.state {
            ButtonState::Press => emulator.key_down(PlayerKey::P2Fire),
            ButtonState::Release => emulator.key_up(PlayerKey::P2Fire),
          }
          Key::Left => match args.state {
            ButtonState::Press => emulator.key_down(PlayerKey::P2Left),
            ButtonState::Release => emulator.key_up(PlayerKey::P2Left),
          }
          Key::Right => match args.state {
            ButtonState::Press => emulator.key_down(PlayerKey::P2Right),
            ButtonState::Release => emulator.key_up(PlayerKey::P2Right),
          }
          Key::A => match args.state {
            ButtonState::Press => emulator.key_down(PlayerKey::P1Left),
            ButtonState::Release => emulator.key_up(PlayerKey::P1Left),
          }
          Key::D => match args.state {
            ButtonState::Press => emulator.key_down(PlayerKey::P1Right),
            ButtonState::Release => emulator.key_up(PlayerKey::P1Right),
          }
          Key::B => match args.state {
            ButtonState::Press => (),
            ButtonState::Release => show_background = !show_background,
          }
          _ => ()
        },
        _ => ()
      }
    }
  }

  Ok(())
}
