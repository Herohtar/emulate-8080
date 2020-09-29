use std::{fs::File, io::Cursor, io::Read, time::Duration, time::Instant};
use rodio::{Device, Sink, Source};

use crate::intel8080::Intel8080;

const CYCLE_TIME: Duration = Duration::from_nanos(480);
const INTERRUPT_INTERVAL: Duration = Duration::from_micros(8333);

const SHOOT: &[u8] = include_bytes!("../sounds/shoot.wav");
const BEAT1: &[u8] = include_bytes!("../sounds/fastinvader1.wav");
const BEAT2: &[u8] = include_bytes!("../sounds/fastinvader2.wav");
const BEAT3: &[u8] = include_bytes!("../sounds/fastinvader3.wav");
const BEAT4: &[u8] = include_bytes!("../sounds/fastinvader4.wav");
const EXPLOSION: &[u8] = include_bytes!("../sounds/explosion.wav");
const INVADER_KILLED: &[u8] = include_bytes!("../sounds/invaderkilled.wav");
const UFO_HIGH_PITCH: &[u8] = include_bytes!("../sounds/ufo_highpitch.wav");
const UFO_LOW_PITCH: &[u8] = include_bytes!("../sounds/ufo_lowpitch.wav");

pub enum PlayerKey {
  Coin,
  Tilt,
  P1Left,
  P1Right,
  P1Fire,
  P1Start,
  P2Left,
  P2Right,
  P2Fire,
  P2Start,
}

pub struct Machine {
  cpu: Intel8080,
  rom_size: u16,
  last_interrupt_time: Option<Instant>,
  next_interrupt: u8,
  last_execution_time: Option<Instant>,
  in_port1: u8,
  in_port2: u8,
  out_port3: u8,
  last_out_port3: u8,
  out_port5: u8,
  last_out_port5: u8,
  shift0: u8,
  shift1: u8,
  shift_offset: u8,
  sound_device: Device,
  ufo_sink: Sink,
}

impl Machine {
  pub fn new() -> Self {
    let device = rodio::default_output_device().unwrap();
    let sink = Sink::new(&device);
    sink.pause();
    let sound = rodio::Decoder::new(Cursor::new(UFO_HIGH_PITCH)).unwrap();
    sink.append(sound.repeat_infinite());

    Machine {
      cpu: {
        let mut cpu = Intel8080::new();
        cpu.input_ports[0] = 0b1110;
        cpu.input_ports[1] = 0b1000;
        cpu.input_ports[2] = 0b1011; // "Easy mode" -- start with 6 lives, gain a new life every 1000 points
        cpu
      },
      rom_size: 0,
      last_interrupt_time: None,
      next_interrupt: 1,
      last_execution_time: None,
      in_port1: 0,
      in_port2: 0,
      out_port3: 0,
      last_out_port3: 0,
      out_port5: 0,
      last_out_port5: 0,
      shift0: 0,
      shift1: 0,
      shift_offset: 0,
      sound_device: device,
      ufo_sink: sink,
    }
  }

  pub fn load_rom_bytes(&mut self, bytes: &[u8]) {
    self.load_rom_bytes_at(bytes, 0);
  }

  pub fn load_rom_bytes_at(&mut self, bytes: &[u8], offset: u16) {
    //TODO: This is not correct if the ROM is loaded in parts
    self.rom_size = bytes.len() as u16;
    self.cpu.memory[offset as usize..offset as usize + self.rom_size as usize].copy_from_slice(bytes);
  }

  #[allow(dead_code)]
  pub fn load_rom(&mut self, path: &str) -> std::io::Result<()> {
    self.load_rom_at(path, 0)?;

    Ok(())
  }

  pub fn load_rom_at(&mut self, path: &str, offset: u16) -> std::io::Result<()> {
    let mut file = File::open(path)?;
    //TODO: This is not correct if the ROM is loaded in parts
    self.rom_size = file.read(&mut self.cpu.memory[offset as usize..])? as u16;

    Ok(())
  }

  #[allow(dead_code)]
  pub fn disassemble_rom(&self) {
    let mut pc = 0;
    while pc < self.rom_size {
      pc += self.cpu.disassemble_8080_op(pc);
    }
  }

  pub fn frame_buffer(&self) -> &[u8] {
    // This is specific to Space Invaders
    &self.cpu.memory[0x2400..0x4000]
  }

  fn space_invaders_in(&self, port: u8) -> u8 {
    match port {
      0 => 0b1110,
      1 => self.in_port1 | 0b1000,
      2 => self.in_port2 | 0b1011, // "Easy mode" -- start with 6 lives, gain a new life every 1000 points
      3 => {
        let v = (self.shift1 as u16) << 8 | self.shift0 as u16;
        ((v >> (8 - self.shift_offset)) & 0x00FF) as u8
      }
      _ => 0x00,
    }
  }

  fn space_invaders_out(&mut self, port: u8, value: u8) {
    match port {
      2 => {
        self.shift_offset = value & 0x07;
        self.update_shift_register();
      }
      3 => self.out_port3 = value,
      4 => {
        self.shift0 = self.shift1;
        self.shift1 = value;
        self.update_shift_register();
      }
      5 => self.out_port5 = value,
      _ => (),
    }
  }

  fn update_shift_register(&mut self) {
    self.cpu.input_ports[3] = {
      let v = (self.shift1 as u16) << 8 | self.shift0 as u16;
      ((v >> (8 - self.shift_offset)) & 0x00FF) as u8
    };
  }

  pub fn execute(&mut self) {
    #[cfg(not(feature = "cputest"))]
    match self.last_interrupt_time {
      Some(time) if time.elapsed() > INTERRUPT_INTERVAL => {
        self.cpu.generate_interrupt(self.next_interrupt);
        self.next_interrupt = match self.next_interrupt {
          1 => 2,
          _ => 1,
        };
        self.last_interrupt_time = Some(Instant::now());
      }
      Some(_) => (),
      None => self.last_interrupt_time = Some(Instant::now()),
    }

    if let Some(time) = self.last_execution_time {
      let cycles_needed = (time.elapsed().as_secs_f64() / CYCLE_TIME.as_secs_f64()).ceil() as u32;
      let mut cycles = 0;
      while cycles < cycles_needed {
        #[cfg(feature = "cputest")]
        if self.cpu.pc == 0x05 {
          self.cpu.special_print();
        }

        cycles += self.cpu.execute_next_instruction() as u32;

        if let Some((out_port, value)) = self.cpu.get_output() {
          self.space_invaders_out(out_port, value);
          self.play_sounds();
        }

        self.last_execution_time = Some(Instant::now());
      }
    } else {
      self.last_execution_time = Some(Instant::now());
    }
  }

  fn play_sounds(&mut self) {
    if self.out_port3 != self.last_out_port3 {
      if self.out_port3 & 0x1 == 0x1 && !(self.last_out_port3 & 0x1 == 0x1) {
        self.ufo_sink.play();
      }
      if self.out_port3 & 0x1 == 0x0 && !(self.last_out_port3 & 0x1 == 0x0) {
        self.ufo_sink.pause();
      }
      if self.out_port3 & 0x2 == 0x2 && !(self.last_out_port3 & 0x2 == 0x2) {
        //TODO: In the actual arcade, shoot is a continuous sound that lasts until the laser hits something
        let sound = rodio::Decoder::new(Cursor::new(SHOOT)).unwrap();
        rodio::play_raw(&self.sound_device, sound.convert_samples());
      }
      if self.out_port3 & 0x4 == 0x4 && !(self.last_out_port3 & 0x4 == 0x4) {
        let sound = rodio::Decoder::new(Cursor::new(EXPLOSION)).unwrap();
        rodio::play_raw(&self.sound_device, sound.convert_samples());
      }
      if self.out_port3 & 0x8 == 0x8 && !(self.last_out_port3 & 0x8 == 0x8) {
        let sound = rodio::Decoder::new(Cursor::new(INVADER_KILLED)).unwrap();
        rodio::play_raw(&self.sound_device, sound.convert_samples());
      }
    }
    if self.out_port5 != self.last_out_port5 {
      if self.out_port5 & 0x1 == 0x1 && !(self.last_out_port5 & 0x1 == 0x1) {
        let sound = rodio::Decoder::new(Cursor::new(BEAT1)).unwrap();
        rodio::play_raw(&self.sound_device, sound.convert_samples());
      }
      if self.out_port5 & 0x2 == 0x2 && !(self.last_out_port5 & 0x2 == 0x2) {
        let sound = rodio::Decoder::new(Cursor::new(BEAT2)).unwrap();
        rodio::play_raw(&self.sound_device, sound.convert_samples());
      }
      if self.out_port5 & 0x4 == 0x4 && !(self.last_out_port5 & 0x4 == 0x4) {
        let sound = rodio::Decoder::new(Cursor::new(BEAT3)).unwrap();
        rodio::play_raw(&self.sound_device, sound.convert_samples());
      }
      if self.out_port5 & 0x8 == 0x8 && !(self.last_out_port5 & 0x8 == 0x8) {
        let sound = rodio::Decoder::new(Cursor::new(BEAT4)).unwrap();
        rodio::play_raw(&self.sound_device, sound.convert_samples());
      }
      if self.out_port5 & 0x10 == 0x10 && !(self.last_out_port5 & 0x10 == 0x10) {
        let sound = rodio::Decoder::new(Cursor::new(UFO_LOW_PITCH)).unwrap();
        rodio::play_raw(&self.sound_device, sound.convert_samples());
      }
    }
    self.last_out_port3 = self.out_port3;
    self.last_out_port5 = self.out_port5;
  }

  pub fn key_down(&mut self, key: PlayerKey) {
    match key {
        PlayerKey::Coin => self.cpu.input_ports[1] |= 0x01,
        PlayerKey::Tilt => self.cpu.input_ports[2] |= 0x04,
        PlayerKey::P1Left => self.cpu.input_ports[1] |= 0x20,
        PlayerKey::P1Right => self.cpu.input_ports[1] |= 0x40,
        PlayerKey::P1Fire => self.cpu.input_ports[1] |= 0x10,
        PlayerKey::P1Start => self.cpu.input_ports[1] |= 0x04,
        PlayerKey::P2Left => self.cpu.input_ports[2] |= 0x20,
        PlayerKey::P2Right => self.cpu.input_ports[2] |= 0x40,
        PlayerKey::P2Fire => self.cpu.input_ports[2] |= 0x10,
        PlayerKey::P2Start => self.cpu.input_ports[1] |= 0x02,
    }
  }

  pub fn key_up(&mut self, key: PlayerKey) {
    match key {
        PlayerKey::Coin => self.cpu.input_ports[1] &= !0x01,
        PlayerKey::Tilt => self.cpu.input_ports[2] &= !0x04,
        PlayerKey::P1Left => self.cpu.input_ports[1] &= !0x20,
        PlayerKey::P1Right => self.cpu.input_ports[1] &= !0x40,
        PlayerKey::P1Fire => self.cpu.input_ports[1] &= !0x10,
        PlayerKey::P1Start => self.cpu.input_ports[1] &= !0x04,
        PlayerKey::P2Left => self.cpu.input_ports[2] &= !0x20,
        PlayerKey::P2Right => self.cpu.input_ports[2] &= !0x40,
        PlayerKey::P2Fire => self.cpu.input_ports[2] &= !0x10,
        PlayerKey::P2Start => self.cpu.input_ports[1] &= !0x02,
    }
  }
}
