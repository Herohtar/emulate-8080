#[allow(unused_imports)]
use std::io::Write;

enum Sign {
  Positive,
  Negative,
}

enum Parity {
  Even,
  Odd,
}

enum Register {
  A,
  B,
  C,
  D,
  E,
  H,
  L,
  M,
}

struct ConditionCodes {
  z: bool,
  s: Sign,
  p: Parity,
  cy: bool,
  ac: bool,
}

#[derive(Copy, Clone)]
enum Interrupts {
  Disabled,
  PreEnabled,
  Enabled,
}

pub struct Intel8080 {
  //TODO: See if there is a way to expose these without making them public
  pub a: u8,
  b: u8,
  c: u8,
  d: u8,
  e: u8,
  h: u8,
  l: u8,
  sp: u16,
  pub pc: u16,
  pub memory: [u8; 0x10000],
  cc: ConditionCodes,
  interrupts: Interrupts,
  halted: bool,
  has_output: bool,
  output_port: u8,
  pub input_ports: [u8; 256],
}

impl Intel8080 {
  pub fn new() -> Self {
    Intel8080 {
      a: 0,
      b: 0,
      c: 0,
      d: 0,
      e: 0,
      h: 0,
      l: 0,
      sp: 0,
      pc: 0,
      memory: [0; 0x10000],
      cc: ConditionCodes {
        z: true,
        s: Sign::Positive,
        p: Parity::Even,
        cy: false,
        ac: false,
      },
      interrupts: Interrupts::Disabled,
      halted: false,
      has_output: false,
      output_port: 0,
      input_ports: [0; 256],
    }
  }

  fn add(&mut self, value: u8) {
    let (result, overflow) = self.a.overflowing_add(value);
    self.cc.z = result == 0;
    self.cc.s = get_sign(result);
    self.cc.p = get_parity(result);
    self.cc.cy = overflow;
    self.cc.ac = (self.a & 0x10) ^ (value & 0x10) ^ (result & 0x10) == 0x10;
    self.a = result;
  }

  fn add_carry(&mut self, value: u8) {
    let before = self.a;
    let mut result = before as u16 + value as u16;
    if self.cc.cy {
      result += 1;
    }
    self.a = (result & 0x00FF) as u8;
    self.cc.z = self.a == 0;
    self.cc.s = get_sign(self.a);
    self.cc.p = get_parity(self.a);
    self.cc.cy = (result & 0x100) == 0x100;
    self.cc.ac = (before & 0x10) ^ (value & 0x10) ^ (self.a & 0x10) == 0x10;
  }

  fn subtract(&mut self, value: u8) {
    let (result, overflow) = self.a.overflowing_sub(value);
    self.cc.z = result == 0;
    self.cc.s = get_sign(result);
    self.cc.p = get_parity(result);
    self.cc.cy = overflow;
    self.cc.ac = (self.a & 0x10) ^ (value & 0x10) ^ (result & 0x10) == 0x10;
    self.a = result;
  }

  fn subtract_borrow(&mut self, value: u8) {
    let before = self.a;
    let mut result = (before as u16).wrapping_sub(value as u16);
    if self.cc.cy {
      result = result.wrapping_sub(1);
    }
    self.a = (result & 0x00FF) as u8;
    self.cc.z = self.a == 0;
    self.cc.s = get_sign(self.a);
    self.cc.p = get_parity(self.a);
    self.cc.cy = (result & 0x100) == 0x100;
    self.cc.ac = (before & 0x10) ^ (value & 0x10) ^ (self.a & 0x10) == 0x10;
  }

  fn compare(&mut self, value: u8) {
    let (result, overflow) = self.a.overflowing_sub(value);
    self.cc.z = result == 0;
    self.cc.s = get_sign(result);
    self.cc.p = get_parity(result);
    self.cc.cy = overflow;
    self.cc.ac = (self.a & 0x10) ^ (value & 0x10) ^ (result & 0x10) == 0x10;
  }

  fn increment(&mut self, register: Register) {
    let register = match register {
      Register::A => &mut self.a,
      Register::B => &mut self.b,
      Register::C => &mut self.c,
      Register::D => &mut self.d,
      Register::E => &mut self.e,
      Register::H => &mut self.h,
      Register::L => &mut self.l,
      Register::M => &mut self.memory[((self.h as u16) << 8 | self.l as u16) as usize],
    };
    let before = *register;
    *register = register.wrapping_add(1);
    self.cc.z = *register == 0;
    self.cc.s = get_sign(*register);
    self.cc.p = get_parity(*register);
    self.cc.ac = (before & 0x10) ^ (*register & 0x10) == 0x10;
  }

  fn decrement(&mut self, register: Register) {
    let register = match register {
      Register::A => &mut self.a,
      Register::B => &mut self.b,
      Register::C => &mut self.c,
      Register::D => &mut self.d,
      Register::E => &mut self.e,
      Register::H => &mut self.h,
      Register::L => &mut self.l,
      Register::M => &mut self.memory[((self.h as u16) << 8 | self.l as u16) as usize],
    };
    let before = *register;
    *register = register.wrapping_sub(1);
    self.cc.z = *register == 0;
    self.cc.s = get_sign(*register);
    self.cc.p = get_parity(*register);
    self.cc.ac = (before & 0x10) ^ (*register & 0x10) == 0x10;
  }

  fn push(&mut self, high: u8, low: u8) {
    self.write_memory(self.sp - 1, high);
    self.write_memory(self.sp - 2, low);
    self.sp -= 2;
  }

  fn pop(&mut self) -> (u8, u8) {
    let high = self.memory[self.sp as usize + 1];
    let low = self.memory[self.sp as usize];
    self.sp += 2;

    (high, low)
  }

  fn call(&mut self, address: &[u8]) {
    let return_address = self.pc + 2; // Next instruction after this one
    self.push((return_address >> 8) as u8, (return_address & 0x00FF) as u8);
    self.pc = (address[1] as u16) << 8 | address[0] as u16;
  }

  fn ret(&mut self) {
    let (high, low) = self.pop();
    self.pc = (high as u16) << 8 | low as u16;
  }

  fn set_logic_flags(&mut self) {
    self.cc.z = self.a == 0;
    self.cc.s = get_sign(self.a);
    self.cc.p = get_parity(self.a);
    self.cc.cy = false;
    self.cc.ac = false;
  }

  fn read_from_hl(&self) -> u8 {
    self.memory[((self.h as u16) << 8 | self.l as u16) as usize]
  }

  fn write_to_hl(&mut self, data: u8) {
    self.write_memory((self.h as u16) << 8 | self.l as u16, data);
  }

  fn write_memory(&mut self, address: u16, data: u8) {
    // These limits are specific to Space Invaders
    #[cfg(not(feature = "cputest"))]
    if address < 0x2000 {
      println!("Attempted write to ROM {:0>4X}", address);
      return;
    } else if address >= 0x4000 {
      print!("Attempted to write outside of Space Invaders RAM {:0>4X}: ", address);
      self.disassemble_8080_op(self.pc - 1);
      return;
    }

    self.memory[address as usize] = data;
  }

  pub fn generate_interrupt(&mut self, number: u8) {
    match self.interrupts {
      Interrupts::Enabled => {
        self.push((self.pc >> 8) as u8, (self.pc & 0x00FF) as u8);
        self.pc = 8 * number as u16;
        self.interrupts = Interrupts::Disabled;
      },
      _ => (),
    }
  }

  #[cfg(feature = "cputest")]
  pub fn special_print(&mut self) {
    match self.c {
      9 => {
        // Message starts with ASCII sequence 0x0C 0x0D 0x0A (NPFF, CR, LF)
        // Add 3 to the initial offset to skip them
        let start = ((self.d as u16) << 8 | self.e as u16) as usize + 3;
        let mut end = start;
        // Message ends with '$'
        while self.memory[end] != '$' as u8 {
          end += 1;
        }
        let data = std::str::from_utf8(&self.memory[start..end]).unwrap();
        eprint!("{}", data);
        std::io::stderr().flush().unwrap();
      }
      2 => {
        // Character
        eprint!("{}", self.e as char);
        std::io::stderr().flush().unwrap();
      }
      other => panic!("Unknown special command: {}", other)
    }
  }

  pub fn get_output(&mut self) -> Option<(u8, u8)> {
    match self.has_output {
      true => {
        self.has_output = false; //TODO: This line is only necessary as long as IN is being handled externally
        Some((self.output_port, self.a))
      }
      false => None,
    }
  }

  pub fn execute_next_instruction(&mut self) -> u8 {
    if self.halted {
      return 0;
    }

    // If interrupts were set to be enabled last time, fully enable them this time around so they will be available after this instruction executes
    self.interrupts = match self.interrupts {
      Interrupts::PreEnabled => Interrupts::Enabled,
      other => other,
    };
    self.has_output = false;

    #[cfg(feature = "printops")]
    self.disassemble_8080_op(self.pc as usize);

    let mut opcode = [0; 3];
    opcode.copy_from_slice(&self.memory[self.pc as usize..self.pc as usize + 3]);
    self.pc += 1;

    match opcode[0] {
      0x00 => 4, // NOP
      0x01 => { // LXI B, D16
        self.c = opcode[1];
        self.b = opcode[2];
        self.pc += 2;
        11
      }
      0x02 => { // STAX B
        self.memory[((self.b as u16) << 8 | self.c as u16) as usize] = self.a;
        7
      }
      0x03 => { // INX B
        self.c = self.c.wrapping_add(1);
        if self.c == 0 {
          self.b = self.b.wrapping_add(1);
        }
        5
      }
      0x04 => { // INR B
        self.increment(Register::B);
        5
      }
      0x05 => { // DCR B
        self.decrement(Register::B);
        5
      }
      0x06 => { // MVI B, D8
        self.b = opcode[1];
        self.pc += 1;
        7
      }
      0x07 => { // RLC
        self.cc.cy = self.a & 0x80 == 0x80;
        self.a = self.a.rotate_left(1);
        4
      }
      // No 0x08
      0x09 => { // DAD B
        let hl = (self.h as u16) << 8 | self.l as u16;
        let bc = (self.b as u16) << 8 | self.c as u16;
        let (result, overflow) = hl.overflowing_add(bc);
        self.h = (result >> 8) as u8;
        self.l = (result & 0x00FF) as u8;
        self.cc.cy = overflow;
        10
      }
      0x0a => { // LDAX B
        self.a = self.memory[((self.b as u16) << 8 | self.c as u16) as usize];
        7
      }
      0x0b => { // DCX B
        self.c = self.c.wrapping_sub(1);
        if self.c == 0xFF {
          self.b = self.b.wrapping_sub(1);
        }
        5
      }
      0x0c => { // INR C
        self.increment(Register::C);
        5
      }
      0x0d => { // DCR C
        self.decrement(Register::C);
        5
      }
      0x0e => { // MVI C, D8
        self.c = opcode[1];
        self.pc += 1;
        7
      }
      0x0f => { // RRC
        self.cc.cy = self.a & 0x01 == 1;
        self.a = self.a.rotate_right(1);
        4
      }
      0x11 => { // LXI D, D16
        self.d = opcode[2];
        self.e = opcode[1];
        self.pc += 2;
        10
      }
      0x12 => { // STAX D
        self.memory[((self.d as u16) << 8 | self.e as u16) as usize] = self.a;
        7
      }
      0x13 => { // INX D
        self.e = self.e.wrapping_add(1);
        if self.e == 0 {
          self.d = self.d.wrapping_add(1);
        }
        5
      }
      0x14 => { // INR D
        self.increment(Register::D);
        5
      }
      0x15 => { // DCR D
        self.decrement(Register::D);
        5
      }
      0x16 => { // MVI D, D8
        self.d = opcode[1];
        self.pc += 1;
        7
      }
      0x17 => { // RAL
        let carry = self.cc.cy;
        self.cc.cy = (self.a & 0x80) == 0x80;
        self.a <<= 1;
        self.a |= match carry {
          true => 1,
          false => 0,
        };
        4
      }
      0x19 => { // DAD D
        let hl = (self.h as u16) << 8 | self.l as u16;
        let de = (self.d as u16) << 8 | self.e as u16;
        let (result, overflow) = hl.overflowing_add(de);
        self.h = (result >> 8) as u8;
        self.l = (result & 0x00FF) as u8;
        self.cc.cy = overflow;
        10
      }
      0x1a => { // LDAX D
        self.a = self.memory[((self.d as u16) << 8 | self.e as u16) as usize];
        7
      }
      0x1b => { // DCX D
        self.e = self.e.wrapping_sub(1);
        if self.e == 0xFF {
          self.d = self.d.wrapping_sub(1);
        }
        5
      }
      0x1c => { // INR E
        self.increment(Register::E);
        5
      }
      0x1d => { // DCR E
        self.decrement(Register::E);
        5
      }
      0x1e => { // MVI E, D8
        self.e = opcode[1];
        self.pc += 1;
        7
      }
      0x1f => { // RAR
        let carry = self.cc.cy;
        self.cc.cy = (self.a & 0x01) == 0x01;
        self.a >>= 1;
        self.a |= match carry {
          true => 0x80,
          false => 0x00,
        };
        4
      }
      0x21 => { // LXI H, D16
        self.h = opcode[2];
        self.l = opcode[1];
        self.pc += 2;
        10
      }
      0x22 => { // SHLD
        let address = ((opcode[2] as u16) << 8 | opcode[1] as u16) as usize;
        self.memory[address] = self.l;
        self.memory[address + 1] = self.h;
        self.pc += 2;
        16
      }
      0x23 => { // INX H
        self.l = self.l.wrapping_add(1);
        if self.l == 0 {
          self.h = self.h.wrapping_add(1);
        }
        5
      }
      0x24 => { // INR H
        self.increment(Register::H);
        5
      }
      0x25 => { // DCR H
        self.decrement(Register::H);
        5
      }
      0x26 => { // MVI H, D8
        self.h = opcode[1];
        self.pc += 1;
        7
      }
      0x27 => { // DAA
        if (self.a & 0x0F > 9) || self.cc.ac {
          self.a = self.a.wrapping_add(6);
          self.cc.ac = true;
        } else {
          self.cc.ac = false;
        }
        if (self.a >> 4 > 9) || self.cc.cy {
          self.a = self.a.wrapping_add(6 << 4);
          self.cc.cy = true;
        } else {
          self.cc.cy = false;
        }
        4
      }
      0x29 => { // DAD H
        let hl = (self.h as u16) << 8 | self.l as u16;
        let (result, overflow) = hl.overflowing_add(hl);
        self.h = (result >> 8) as u8;
        self.l = (result & 0x00FF) as u8;
        self.cc.cy = overflow;
        10
      }
      0x2a => { // LHLD adr
        let address = ((opcode[2] as u16) << 8 | opcode[1] as u16) as usize;
        self.l = self.memory[address];
        self.h = self.memory[address + 1];
        self.pc += 2;
        16
      }
      0x2b => { // DCX H
        self.l = self.l.wrapping_sub(1);
        if self.l == 0xFF {
          self.h = self.h.wrapping_sub(1);
        }
        5
      }
      0x2c => { // INR L
        self.increment(Register::L);
        5
      }
      0x2d => { // DCR L
        self.decrement(Register::L);
        5
      }
      0x2e => { // MVI L, D8
        self.l = opcode[1];
        self.pc += 1;
        7
      }
      0x2f => { // CMA
        self.a = !self.a;
        4
      }
      0x31 => { // LXI SP, D16
        self.sp = (opcode[2] as u16) << 8 | opcode[1] as u16;
        self.pc += 2;
        10
      }
      0x32 => { // STA adr
        self.memory[((opcode[2] as u16) << 8 | opcode[1] as u16) as usize] = self.a;
        self.pc += 2;
        13
      }
      0x33 => { // INX SP
        self.sp += 1;
        5
      }
      0x34 => { // INR M
        self.increment(Register::M);
        10
      }
      0x35 => { // DCR M
        self.decrement(Register::M);
        10
      }
      0x36 => { // MVI M, D8
        self.write_to_hl(opcode[1]);
        self.pc += 1;
        10
      }
      0x37 => { // STC
        self.cc.cy = true;
        4
      }
      0x39 => { // DAD SP
        let hl = (self.h as u16) << 8 | self.l as u16;
        let (result, overflow) = hl.overflowing_add(self.sp);
        self.h = (result >> 8) as u8;
        self.l = (result & 0x00FF) as u8;
        self.cc.cy = overflow;
        10
      }
      0x3a => { // LDA adr
        self.a = self.memory[((opcode[2] as u16) << 8 | opcode[1] as u16) as usize];
        self.pc += 2;
        13
      }
      0x3b => { // DCX SP
        self.sp -= 1;
        5
      }
      0x3c => { // INR A
        self.increment(Register::A);
        5
      }
      0x3d => { // DCR A
        self.decrement(Register::A);
        5
      }
      0x3e => { // MVI A, D8
        self.a = opcode[1];
        self.pc += 1;
        7
      }
      0x3f => { // CMC
        self.cc.cy = !self.cc.cy;
        4
      }
      0x41 => { // MOV B, C
        self.b = self.c;
        5
      }
      0x42 => { // MOV B, D
        self.b = self.d;
        5
      }
      0x43 => { // MOV B, E
        self.b = self.e;
        5
      }
      0x44 => { // MOV B, H
        self.b = self.h;
        5
      }
      0x45 => { // MOV B, L
        self.b = self.l;
        5
      }
      0x46 => { // MOV B, M
        self.b = self.read_from_hl();
        7
      }
      0x47 => { // MOV B, A
        self.b = self.a;
        5
      }
      0x48 => { // MOV C, B
        self.c = self.b;
        5
      }
      0x4a => { // MOV C, D
        self.c = self.d;
        5
      }
      0x4b => { // MOV C, E
        self.c = self.e;
        5
      }
      0x4c => { // MOV C, H
        self.c = self.h;
        5
      }
      0x4d => { // MOV C, L
        self.c = self.l;
        5
      }
      0x4e => { // MOV C, M
        self.c = self.read_from_hl();
        7
      }
      0x4f => { // MOV C, A
        self.c = self.a;
        5
      }
      0x50 => { // MOV D, B
        self.d = self.b;
        5
      }
      0x51 => { // MOV D, C
        self.d = self.c;
        5
      }
      0x53 => { // MOV D, E
        self.d = self.e;
        5
      }
      0x54 => { // MOV D, H
        self.d = self.h;
        5
      }
      0x55 => { // MOV D, L
        self.d = self.l;
        5
      }
      0x56 => { // MOV D, M
        self.d = self.read_from_hl();
        7
      }
      0x57 => { // MOV D, A
        self.d = self.a;
        5
      }
      0x58 => { // MOV E, B
        self.e = self.b;
        5
      }
      0x59 => { // MOV E, C
        self.e = self.c;
        5
      }
      0x5a => { // MOV E, D
        self.e = self.d;
        5
      }
      0x5c => { // MOV E, H
        self.e = self.h;
        5
      }
      0x5d => { // MOV E, L
        self.e = self.l;
        5
      }
      0x5e => { // MOV E, M
        self.e = self.read_from_hl();
        7
      }
      0x5f => { // MOV E, A
        self.e = self.a;
        5
      }
      0x60 => { // MOV H, B
        self.h = self.b;
        5
      }
      0x61 => { // MOV H, C
        self.h = self.c;
        5
      }
      0x62 => { // MOV H, D
        self.h = self.d;
        5
      }
      0x63 => { // MOV H, E
        self.h = self.e;
        5
      }
      0x65 => { // MOV H, L
        self.h = self.l;
        5
      }
      0x66 => { // MOV H, M
        self.h = self.read_from_hl();
        7
      }
      0x67 => { // MOV H, A
        self.h = self.a;
        5
      }
      0x68 => { // MOV L, B
        self.l = self.b;
        5
      }
      0x69 => { // MOV L, C
        self.l = self.c;
        5
      }
      0x6a => { // MOV L, D
        self.l = self.d;
        5
      }
      0x6b => { // MOV L, E
        self.l = self.e;
        5
      }
      0x6c => { // MOV L, H
        self.l = self.h;
        5
      }
      0x6e => { // MOV L, M
        self.l = self.read_from_hl();
        7
      }
      0x6f => { // MOV L, A
        self.l = self.a;
        5
      }
      0x70 => { // MOV M, B
        self.write_to_hl(self.b);
        7
      }
      0x71 => { // MOV M, C
        self.write_to_hl(self.c);
        7
      }
      0x72 => { // MOV M, D
        self.write_to_hl(self.d);
        7
      }
      0x73 => { // MOV M, E
        self.write_to_hl(self.e);
        7
      }
      0x74 => { // MOV M, H
        self.write_to_hl(self.h);
        7
      }
      0x75 => { // MOV M, L
        self.write_to_hl(self.l);
        7
      }
      0x76 => { // HLT
        println!("\n\nCPU Halted");
        self.halted = true;
        7
      }
      0x77 => { // MOV M, A
        self.write_to_hl(self.a);
        7
      }
      0x78 => { // MOV A, B
        self.a = self.b;
        5
      }
      0x79 => { // MOV A, C
        self.a = self.c;
        5
      }
      0x7a => { // MOV A, D
        self.a = self.d;
        5
      }
      0x7b => { // MOV A, E
        self.a = self.e;
        5
      }
      0x7c => { // MOV A, H
        self.a = self.h;
        5
      }
      0x7d => { // MOV A, L
        self.a = self.l;
        5
      }
      0x7e => { // MOV A, M
        self.a = self.read_from_hl();
        7
      }
      0x80 => { // ADD B
        self.add(self.b);
        4
      }
      0x81 => { // ADD C
        self.add(self.c);
        4
      }
      0x82 => { // ADD D
        self.add(self.d);
        4
      }
      0x83 => { // ADD E
        self.add(self.e);
        4
      }
      0x84 => { // ADD H
        self.add(self.h);
        4
      }
      0x85 => { // ADD L
        self.add(self.l);
        4
      }
      0x86 => { // ADD M
        self.add(self.read_from_hl());
        7
      }
      0x87 => { // ADD A
        self.add(self.a);
        4
      }
      0x88 => { // ADC B
        self.add_carry(self.b);
        4
      }
      0x89 => { // ADC C
        self.add_carry(self.c);
        4
      }
      0x8a => { // ADC D
        self.add_carry(self.d);
        4
      }
      0x8b => { // ADC E
        self.add_carry(self.e);
        4
      }
      0x8c => { // ADC H
        self.add_carry(self.h);
        4
      }
      0x8d => { // ADC L
        self.add_carry(self.l);
        4
      }
      0x8e => { // ADC M
        self.add_carry(self.read_from_hl());
        7
      }
      0x8f => { // ADC A
        self.add_carry(self.a);
        4
      }
      0x90 => { // SUB B
        self.subtract(self.b);
        4
      }
      0x91 => { // SUB C
        self.subtract(self.c);
        4
      }
      0x92 => { // SUB D
        self.subtract(self.d);
        4
      }
      0x93 => { // SUB E
        self.subtract(self.e);
        4
      }
      0x94 => { // SUB H
        self.subtract(self.h);
        4
      }
      0x95 => { // SUB L
        self.subtract(self.l);
        4
      }
      0x96 => { // SUB M
        self.subtract(self.read_from_hl());
        7
      }
      0x97 => { // SUB A
        self.subtract(self.a);
        4
      }
      0x98 => { // SBB B
        self.subtract_borrow(self.b);
        4
      }
      0x99 => { // SBB C
        self.subtract_borrow(self.c);
        4
      }
      0x9a => { // SBB D
        self.subtract_borrow(self.d);
        4
      }
      0x9b => { // SBB E
        self.subtract_borrow(self.e);
        4
      }
      0x9c => { // SBB H
        self.subtract_borrow(self.h);
        4
      }
      0x9d => { // SBB L
        self.subtract_borrow(self.l);
        4
      }
      0x9e => { // SBB M
        self.subtract_borrow(self.read_from_hl());
        7
      }
      0x9f => { // SBB A
        let value = self.a;
        self.subtract_borrow(value);
        4
      }
      0xa0 => { // ANA B
        self.a &= self.b;
        self.set_logic_flags();
        4
      }
      0xa1 => { // ANA C
        self.a &= self.c;
        self.set_logic_flags();
        4
      }
      0xa2 => { // ANA D
        self.a &= self.d;
        self.set_logic_flags();
        4
      }
      0xa3 => { // ANA E
        self.a &= self.e;
        self.set_logic_flags();
        4
      }
      0xa4 => { // ANA H
        self.a &= self.h;
        self.set_logic_flags();
        4
      }
      0xa5 => { // ANA L
        self.a &= self.l;
        self.set_logic_flags();
        4
      }
      0xa6 => { // ANA M
        self.a &= self.read_from_hl();
        self.set_logic_flags();
        7
      }
      0xa7 => { // ANA A
        self.a &= self.a;
        self.set_logic_flags();
        4
      }
      0xa8 => { // XRA B
        self.a ^= self.b;
        self.set_logic_flags();
        4
      }
      0xa9 => { // XRA C
        self.a ^= self.c;
        self.set_logic_flags();
        4
      }
      0xaa => { // XRA D
        self.a ^= self.d;
        self.set_logic_flags();
        4
      }
      0xab => { // XRA E
        self.a ^= self.e;
        self.set_logic_flags();
        4
      }
      0xac => { // XRA H
        self.a ^= self.h;
        self.set_logic_flags();
        4
      }
      0xad => { // XRA L
        self.a ^= self.l;
        self.set_logic_flags();
        4
      }
      0xae => { // XRA M
        self.a ^= self.read_from_hl();
        self.set_logic_flags();
        7
      }
      0xaf => { // XRA A
        self.a ^= self.a;
        self.set_logic_flags();
        4
      }
      0xb0 => { // ORA B
        self.a |= self.b;
        self.set_logic_flags();
        4
      }
      0xb1 => { // ORA C
        self.a |= self.c;
        self.set_logic_flags();
        4
      }
      0xb2 => { // ORA D
        self.a |= self.d;
        self.set_logic_flags();
        4
      }
      0xb3 => { // ORA E
        self.a |= self.e;
        self.set_logic_flags();
        4
      }
      0xb4 => { // ORA H
        self.a |= self.h;
        self.set_logic_flags();
        4
      }
      0xb5 => { // ORA L
        self.a |= self.l;
        self.set_logic_flags();
        4
      }
      0xb6 => { // ORA M
        self.a |= self.read_from_hl();
        self.set_logic_flags();
        7
      }
      0xb7 => { // ORA A
        self.a |= self.a;
        self.set_logic_flags();
        4
      }
      0xb8 => { // CMP B
        self.compare(self.b);
        4
      }
      0xb9 => { // CMP C
        self.compare(self.c);
        4
      }
      0xba => { // CMP D
        self.compare(self.d);
        4
      }
      0xbb => { // CMP E
        self.compare(self.e);
        4
      }
      0xbc => { // CMP H
        self.compare(self.h);
        4
      }
      0xbd => { // CMP L
        self.compare(self.l);
        4
      }
      0xbe => { // CMP M
        self.compare(self.read_from_hl());
        7
      }
      0xbf => { // CMP A
        self.compare(self.a);
        4
      }
      0xc0 => { // RNZ
        match self.cc.z {
          false => {
            self.ret();
            11
          }
          true => 5,
        }
      }
      0xc1 => { // POP B
        let (high, low) = self.pop();
        self.b = high;
        self.c = low;
        10
      }
      0xc2 => { // JNZ adr
        match self.cc.z {
          false => self.pc = (opcode[2] as u16) << 8 | opcode[1] as u16,
          true => self.pc += 2,
        }
        10
      }
      0xc3 => { // JMP adr
        self.pc = (opcode[2] as u16) << 8 | opcode[1] as u16;
        10
      }
      0xc4 => { // CNZ adr
        match self.cc.z {
          false => {
            self.call(&opcode[1..]);
            17
          }
          true => {
            self.pc += 2;
            11
          }
        }
      }
      0xc5 => { // PUSH B
        self.push(self.b, self.c);
        11
      }
      0xc6 => { // ADI D8
        self.add(opcode[1]);
        self.pc += 1;
        7
      }
      0xc8 => { // RZ
        match self.cc.z {
          true => {
            self.ret();
            11
          }
          false => 5,
        }
      }
      0xc9 => { // RET
        self.ret();
        10
      }
      0xca => { // JZ adr
        match self.cc.z {
          true => self.pc = (opcode[2] as u16) << 8 | opcode[1] as u16,
          false => self.pc += 2,
        }
        10
      }
      0xcc => { // CZ adr
        match self.cc.z {
          true => {
            self.call(&opcode[1..]);
            17
          }
          false => {
            self.pc += 2;
            11
          }
        }
      }
      0xcd => { // CALL adr
        self.call(&opcode[1..]);
        17
      }
      0xce => { // ACI D8
        self.add_carry(opcode[1]);
        self.pc += 1;
        7
      }
      0xd0 => { // RNC
        match self.cc.cy {
          false => {
            self.ret();
            11
          }
          true => 5,
        }
      }
      0xd1 => { // POP D
        let (high, low) = self.pop();
        self.d = high;
        self.e = low;
        10
      }
      0xd2 => { // JNC adr
        if !self.cc.cy {
          self.pc = (opcode[2] as u16) << 8 | opcode[1] as u16;
        } else {
          self.pc += 2;
        }
        10
      }
      0xd3 => { // OUT D8
        self.output_port = opcode[1];
        self.has_output = true;
        self.pc += 1;
        10
      }
      0xd4 => { // CNC adr
        match self.cc.cy {
          false => {
            self.call(&opcode[1..]);
            17
          }
          true => {
            self.pc += 2;
            11
          }
        }
      }
      0xd5 => { // PUSH D
        self.push(self.d, self.e);
        11
      }
      0xd6 => { // SUI D8
        self.subtract(opcode[1]);
        self.pc += 1;
        7
      }
      0xd8 => { // RC
        match self.cc.cy {
          true => {
            self.ret();
            11
          }
          false => 5,
        }
      }
      0xda => { // JC adr
        if self.cc.cy {
          self.pc = (opcode[2] as u16) << 8 | opcode[1] as u16;
        } else {
          self.pc += 2;
        }
        10
      }
      0xdb => { // IN D8
        //TODO: Figure out if there is a way to implement IN here. API?
        println!("**Incomplete opcode 0xDB**");
        self.pc += 1;
        10
      }
      0xdc => { // CC adr
        match self.cc.cy {
          true => {
            self.call(&opcode[1..]);
            17
          }
          false => {
            self.pc += 2;
            11
          }
        }
      }
      0xde => { // SBI D8
        self.subtract_borrow(opcode[1]);
        self.pc += 1;
        7
      }
      0xe0 => { // RPO
        match self.cc.p {
          Parity::Odd => {
            self.ret();
            11
          }
          Parity::Even => 5,
        }
      }
      0xe1 => { // POP H
        let (high, low) = self.pop();
        self.h = high;
        self.l = low;
        10
      }
      0xe2 => { // JPO adr
        match self.cc.p {
          Parity::Odd => self.pc = (opcode[2] as u16) << 8 | opcode[1] as u16,
          Parity::Even => self.pc += 2,
        }
        10
      }
      0xe3 => { // XTHL
        std::mem::swap(&mut self.l, &mut self.memory[self.sp as usize]);
        std::mem::swap(&mut self.h, &mut self.memory[self.sp as usize + 1]);
        18
      }
      0xe4 => { // CPO adr
        match self.cc.p {
          Parity::Odd => {
            self.call(&opcode[1..]);
            17
          }
          Parity::Even => {
            self.pc += 2;
            11
          }
        }
      }
      0xe5 => { // PUSH H
        self.push(self.h, self.l);
        11
      }
      0xe6 => { // ANI D8
        self.a &= opcode[1];
        self.set_logic_flags();
        self.pc += 1;
        7
      }
      0xe8 => { // RPE
        match self.cc.p {
          Parity::Even => {
            self.ret();
            11
          }
          Parity::Odd => 5,
        }
      }
      0xe9 => { // PCHL
        self.pc = (self.h as u16) << 8 | self.l as u16;
        5
      }
      0xea => { // JPE adr
        match self.cc.p {
          Parity::Even => {
            self.pc = (opcode[2] as u16) << 8 | opcode[1] as u16;
          }
          Parity::Odd => self.pc += 2
        }
        10
      }
      0xeb => { // XCHG
        std::mem::swap(&mut self.h, &mut self.d);
        std::mem::swap(&mut self.l, &mut self.e);
        4
      }
      0xec => { // CPE adr
        match self.cc.p {
          Parity::Even => {
            self.call(&opcode[1..]);
            17
          }
          Parity::Odd => {
            self.pc += 2;
            11
          }
        }
      }
      0xee => { // XRI D8
        self.a ^= opcode[1];
        self.set_logic_flags();
        self.pc += 1;
        7
      }
      0xf0 => { // RP
        match self.cc.s {
          Sign::Positive => {
            self.ret();
            11
          }
          Sign::Negative => 5,
        }
      }
      0xf1 => { // POP PSW
        let (acc, flags) = self.pop();
        self.a = acc;
        self.cc.z = flags & 0b1 == 0;
        self.cc.s = match flags & 0b10 {
          0 => Sign::Positive,
          _ => Sign::Negative,
        };
        self.cc.p = match flags & 0b100 {
          0 => Parity::Even,
          _ => Parity::Odd,
        };
        self.cc.cy = flags & 0b1000 == 0b1000;
        self.cc.ac = flags & 0b10000 == 0b10000;
        10
      }
      0xf2 => { // JP adr
        match self.cc.s {
          Sign::Positive => self.pc = (opcode[2] as u16) << 8 | opcode[1] as u16,
          Sign::Negative => self.pc += 2,
        }
        10
      }
      0xf3 => { // DI
        // Interrupts are disabled immediately following execution of the DI instruction
        self.interrupts = Interrupts::Disabled;
        4
      }
      0xf4 => { // CP adr
        match self.cc.s {
          Sign::Positive => {
            self.call(&opcode[1..]);
            17
          }
          Sign::Negative => {
            self.pc += 2;
            11
          }
        }
      }
      0xf5 => { // PUSH PSW
        let mut flags = 0u8;
        flags |= match self.cc.z {
          true => 0,
          false => 1,
        };
        flags |= match self.cc.s {
          Sign::Negative => 0b10,
          Sign::Positive => 0b00,
        };
        flags |= match self.cc.p {
          Parity::Odd => 0b100,
          Parity::Even => 0b000,
        };
        flags |= match self.cc.cy {
          true => 0b1000,
          false => 0b0000,
        };
        flags |= match self.cc.ac {
          true => 0b10000,
          false => 0b00000,
        };
        self.push(self.a, flags);
        11
      }
      0xf6 => { // ORI D8
        self.a |= opcode[1];
        self.set_logic_flags();
        self.pc += 1;
        7
      }
      0xf8 => { // RM
        match self.cc.s {
          Sign::Negative => {
            self.ret();
            11
          }
          Sign::Positive => 5,
        }
      }
      0xf9 => { // SPHL
        self.sp = (self.h as u16) << 8 | self.l as u16;
        5
      }
      0xfa => { // JM adr
        match self.cc.s {
          Sign::Negative => self.pc = (opcode[2] as u16) << 8 | opcode[1] as u16,
          Sign::Positive => self.pc += 2,
        }
        10
      }
      0xfb => { // EI
        // Interrupts are enabled following execution of the *next* instruction
        self.interrupts = Interrupts::PreEnabled;
        4
      }
      0xfc => { // CM adr
        match self.cc.s {
          Sign::Negative => {
            self.call(&opcode[1..]);
            17
          }
          Sign::Positive => {
            self.pc += 2;
            11
          }
        }
      }
      0xfe => { // CPI D8
        self.compare(opcode[1]);
        self.pc += 1;
        7
      }
      other => {
        //self.disassemble_8080_op(self.pc as usize - 1);
        panic!("Unimplemented opcode: 0x{:0>2X}", other);
      }
    }
  }

  pub fn disassemble_8080_op(&self, pc: u16) -> u16 {
    let pc = pc as usize;
    print!("{:0>4X} ", pc);
    match self.memory[pc] {
      0x00 => {
        println!("NOP");
        1
      }
      0x01 => {
        println!("LXI    B, #${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0x02 => {
        println!("STAX   B");
        1
      }
      0x03 => {
        println!("INX    B");
        1
      }
      0x04 => {
        println!("INR    B");
        1
      }
      0x05 => {
        println!("DCR    B");
        1
      }
      0x06 => {
        println!("MVI    B, #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0x07 => {
        println!("RLC");
        1
      }
      // No 0x08
      0x09 => {
        println!("DAD    B");
        1
      }
      0x0a => {
        println!("LDAX   B");
        1
      }
      0x0b => {
        println!("DCX    B");
        1
      }
      0x0c => {
        println!("INR    C");
        1
      }
      0x0d => {
        println!("DCR    C");
        1
      }
      0x0e => {
        println!("MVI    C, #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0x0f => {
        println!("RRC");
        1
      }
      // No 0x10
      0x11 => {
        println!("LXI    D, #${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0x12 => {
        println!("STAX   D");
        1
      }
      0x13 => {
        println!("INX    D");
        1
      }
      0x14 => {
        println!("INR    D");
        1
      }
      0x15 => {
        println!("DCR    D");
        1
      }
      0x16 => {
        println!("MVI    D, #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0x17 => {
        println!("RAL");
        1
      }
      // No 0x18
      0x19 => {
        println!("DAD    D");
        1
      }
      0x1a => {
        println!("LDAX   D");
        1
      }
      0x1b => {
        println!("DCX    D");
        1
      }
      0x1c => {
        println!("INR    E");
        1
      }
      0x1d => {
        println!("DCR    E");
        1
      }
      0x1e => {
        println!("MVI    E, #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0x1f => {
        println!("RAR");
        1
      }
      // No 0x20
      0x21 => {
        println!("LXI    H, #${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0x22 => {
        println!("SHLD   ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0x23 => {
        println!("INX    H");
        1
      }
      0x24 => {
        println!("INR    H");
        1
      }
      0x25 => {
        println!("DCR    H");
        1
      }
      0x26 => {
        println!("MVI    H, #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0x27 => {
        println!("DAA");
        1
      }
      // No 0x28
      0x29 => {
        println!("DAD    H");
        1
      }
      0x2a => {
        println!("LHLD   ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0x2b => {
        println!("DCX    H");
        1
      }
      0x2c => {
        println!("INR    L");
        1
      }
      0x2d => {
        println!("DCR    L");
        1
      }
      0x2e => {
        println!("MVI    L, #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0x2f => {
        println!("CMA");
        1
      }
      // No 0x30
      0x31 => {
        println!("LXI    SP, #${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0x32 => {
        println!("STA    ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0x33 => {
        println!("INX    SP");
        1
      }
      0x34 => {
        println!("INR    M");
        1
      }
      0x35 => {
        println!("DCR    M");
        1
      }
      0x36 => {
        println!("MVI    M, #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0x37 => {
        println!("STC");
        1
      }
      // No 0x38
      0x39 => {
        println!("DAD    SP");
        1
      }
      0x3a => {
        println!("LDA    ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0x3b => {
        println!("DCX    SP");
        1
      }
      0x3c => {
        println!("INR    A");
        1
      }
      0x3d => {
        println!("DCR    A");
        1
      }
      0x3e => {
        println!("MVI    A, #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0x3f => {
        println!("CMC");
        1
      }
      0x40 => {
        println!("MOV    B, B");
        1
      }
      0x41 => {
        println!("MOV    B, C");
        1
      }
      0x42 => {
        println!("MOV    B, D");
        1
      }
      0x43 => {
        println!("MOV    B, E");
        1
      }
      0x44 => {
        println!("MOV    B, H");
        1
      }
      0x45 => {
        println!("MOV    B, L");
        1
      }
      0x46 => {
        println!("MOV    B, M");
        1
      }
      0x47 => {
        println!("MOV    B, A");
        1
      }
      0x48 => {
        println!("MOV    C, B");
        1
      }
      0x49 => {
        println!("MOV    C, C");
        1
      }
      0x4a => {
        println!("MOV    C, D");
        1
      }
      0x4b => {
        println!("MOV    C, E");
        1
      }
      0x4c => {
        println!("MOV    C, H");
        1
      }
      0x4d => {
        println!("MOV    C, L");
        1
      }
      0x4e => {
        println!("MOV    C, M");
        1
      }
      0x4f => {
        println!("MOV    C, A");
        1
      }
      0x50 => {
        println!("MOV    D, B");
        1
      }
      0x51 => {
        println!("MOV    D, C");
        1
      }
      0x52 => {
        println!("MOV    D, D");
        1
      }
      0x53 => {
        println!("MOV    D, E");
        1
      }
      0x54 => {
        println!("MOV    D, H");
        1
      }
      0x55 => {
        println!("MOV    D, L");
        1
      }
      0x56 => {
        println!("MOV    D, M");
        1
      }
      0x57 => {
        println!("MOV    D, A");
        1
      }
      0x58 => {
        println!("MOV    E, B");
        1
      }
      0x59 => {
        println!("MOV    E, C");
        1
      }
      0x5a => {
        println!("MOV    E, D");
        1
      }
      0x5b => {
        println!("MOV    E, E");
        1
      }
      0x5c => {
        println!("MOV    E, H");
        1
      }
      0x5d => {
        println!("MOV    E, L");
        1
      }
      0x5e => {
        println!("MOV    E, M");
        1
      }
      0x5f => {
        println!("MOV    E, A");
        1
      }
      0x60 => {
        println!("MOV    H, B");
        1
      }
      0x61 => {
        println!("MOV    H, C");
        1
      }
      0x62 => {
        println!("MOV    H, D");
        1
      }
      0x63 => {
        println!("MOV    H, E");
        1
      }
      0x64 => {
        println!("MOV    H, H");
        1
      }
      0x65 => {
        println!("MOV    H, L");
        1
      }
      0x66 => {
        println!("MOV    H, M");
        1
      }
      0x67 => {
        println!("MOV    H, A");
        1
      }
      0x68 => {
        println!("MOV    L, B");
        1
      }
      0x69 => {
        println!("MOV    L, C");
        1
      }
      0x6a => {
        println!("MOV    L, D");
        1
      }
      0x6b => {
        println!("MOV    L, E");
        1
      }
      0x6c => {
        println!("MOV    L, H");
        1
      }
      0x6d => {
        println!("MOV    L, L");
        1
      }
      0x6e => {
        println!("MOV    L, M");
        1
      }
      0x6f => {
        println!("MOV    L, A");
        1
      }
      0x70 => {
        println!("MOV    M, B");
        1
      }
      0x71 => {
        println!("MOV    M, C");
        1
      }
      0x72 => {
        println!("MOV    M, D");
        1
      }
      0x73 => {
        println!("MOV    M, E");
        1
      }
      0x74 => {
        println!("MOV    M, H");
        1
      }
      0x75 => {
        println!("MOV    M, L");
        1
      }
      0x76 => {
        println!("HLT");
        1
      }
      0x77 => {
        println!("MOV    M, A");
        1
      }
      0x78 => {
        println!("MOV    A, B");
        1
      }
      0x79 => {
        println!("MOV    A, C");
        1
      }
      0x7a => {
        println!("MOV    A, D");
        1
      }
      0x7b => {
        println!("MOV    A, E");
        1
      }
      0x7c => {
        println!("MOV    A, H");
        1
      }
      0x7d => {
        println!("MOV    A, L");
        1
      }
      0x7e => {
        println!("MOV    A, M");
        1
      }
      0x7f => {
        println!("MOV    A, A");
        1
      }
      0x80 => {
        println!("ADD    B");
        1
      }
      0x81 => {
        println!("ADD    C");
        1
      }
      0x82 => {
        println!("ADD    D");
        1
      }
      0x83 => {
        println!("ADD    E");
        1
      }
      0x84 => {
        println!("ADD    H");
        1
      }
      0x85 => {
        println!("ADD    L");
        1
      }
      0x86 => {
        println!("ADD    M");
        1
      }
      0x87 => {
        println!("ADD    A");
        1
      }
      0x88 => {
        println!("ADC    B");
        1
      }
      0x89 => {
        println!("ADC    C");
        1
      }
      0x8a => {
        println!("ADC    D");
        1
      }
      0x8b => {
        println!("ADC    E");
        1
      }
      0x8c => {
        println!("ADC    H");
        1
      }
      0x8d => {
        println!("ADC    L");
        1
      }
      0x8e => {
        println!("ADC    M");
        1
      }
      0x8f => {
        println!("ADC    A");
        1
      }
      0x90 => {
        println!("SUB    B");
        1
      }
      0x91 => {
        println!("SUB    C");
        1
      }
      0x92 => {
        println!("SUB    D");
        1
      }
      0x93 => {
        println!("SUB    E");
        1
      }
      0x94 => {
        println!("SUB    H");
        1
      }
      0x95 => {
        println!("SUB    L");
        1
      }
      0x96 => {
        println!("SUB    M");
        1
      }
      0x97 => {
        println!("SUB    A");
        1
      }
      0x98 => {
        println!("SBB    B");
        1
      }
      0x99 => {
        println!("SBB    C");
        1
      }
      0x9a => {
        println!("SBB    D");
        1
      }
      0x9b => {
        println!("SBB    E");
        1
      }
      0x9c => {
        println!("SBB    H");
        1
      }
      0x9d => {
        println!("SBB    L");
        1
      }
      0x9e => {
        println!("SBB    M");
        1
      }
      0x9f => {
        println!("SBB    A");
        1
      }
      0xa0 => {
        println!("ANA    B");
        1
      }
      0xa1 => {
        println!("ANA    C");
        1
      }
      0xa2 => {
        println!("ANA    D");
        1
      }
      0xa3 => {
        println!("ANA    E");
        1
      }
      0xa4 => {
        println!("ANA    H");
        1
      }
      0xa5 => {
        println!("ANA    L");
        1
      }
      0xa6 => {
        println!("ANA    M");
        1
      }
      0xa7 => {
        println!("ANA    A");
        1
      }
      0xa8 => {
        println!("XRA    B");
        1
      }
      0xa9 => {
        println!("XRA    C");
        1
      }
      0xaa => {
        println!("XRA    D");
        1
      }
      0xab => {
        println!("XRA    E");
        1
      }
      0xac => {
        println!("XRA    H");
        1
      }
      0xad => {
        println!("XRA    L");
        1
      }
      0xae => {
        println!("XRA    M");
        1
      }
      0xaf => {
        println!("XRA    A");
        1
      }
      0xb0 => {
        println!("ORA    B");
        1
      }
      0xb1 => {
        println!("ORA    C");
        1
      }
      0xb2 => {
        println!("ORA    D");
        1
      }
      0xb3 => {
        println!("ORA    E");
        1
      }
      0xb4 => {
        println!("ORA    H");
        1
      }
      0xb5 => {
        println!("ORA    L");
        1
      }
      0xb6 => {
        println!("ORA    M");
        1
      }
      0xb7 => {
        println!("ORA    A");
        1
      }
      0xb8 => {
        println!("CMP    B");
        1
      }
      0xb9 => {
        println!("CMP    C");
        1
      }
      0xba => {
        println!("CMP    D");
        1
      }
      0xbb => {
        println!("CMP    E");
        1
      }
      0xbc => {
        println!("CMP    H");
        1
      }
      0xbd => {
        println!("CMP    L");
        1
      }
      0xbe => {
        println!("CMP    M");
        1
      }
      0xbf => {
        println!("CMP    A");
        1
      }
      0xc0 => {
        println!("RNZ");
        1
      }
      0xc1 => {
        println!("POP    B");
        1
      }
      0xc2 => {
        println!("JNZ    ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xc3 => {
        println!("JMP    ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xc4 => {
        println!("CNZ    ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xc5 => {
        println!("PUSH   B");
        1
      }
      0xc6 => {
        println!("ADI    #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0xc7 => {
        println!("RST    0");
        1
      }
      0xc8 => {
        println!("RZ");
        1
      }
      0xc9 => {
        println!("RET");
        1
      }
      0xca => {
        println!("JZ     ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      // No 0xcb
      0xcc => {
        println!("CZ     ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xcd => {
        println!("CALL   ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc +1]);
        3
      }
      0xce => {
        println!("ACI    #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0xcf => {
        println!("RST    1");
        1
      }
      0xd0 => {
        println!("RNC");
        1
      }
      0xd1 => {
        println!("POP    D");
        1
      }
      0xd2 => {
        println!("JNC    ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xd3 => {
        println!("OUT    #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0xd4 => {
        println!("CNC    ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xd5 => {
        println!("PUSH   D");
        1
      }
      0xd6 => {
        println!("SUI    #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0xd7 => {
        println!("RST    2");
        1
      }
      0xd8 => {
        println!("RC");
        1
      }
      // No 0xd9
      0xda => {
        println!("JC     ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xdb => {
        println!("IN     #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0xdc => {
        println!("CC     ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      // No 0xdd
      0xde => {
        println!("SBI    #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0xdf => {
        println!("RST    3");
        1
      }
      0xe0 => {
        println!("RPO");
        1
      }
      0xe1 => {
        println!("POP    H");
        1
      }
      0xe2 => {
        println!("JPO    ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xe3 => {
        println!("XTHL");
        1
      }
      0xe4 => {
        println!("CPO    ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xe5 => {
        println!("PUSH   H");
        1
      }
      0xe6 => {
        println!("ANI    #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0xe7 => {
        println!("RST    4");
        1
      }
      0xe8 => {
        println!("RPE");
        1
      }
      0xe9 => {
        println!("PCHL");
        1
      }
      0xea => {
        println!("JPE    ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xeb => {
        println!("XCHG");
        1
      }
      0xec => {
        println!("CPE    ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      // No 0xed
      0xee => {
        println!("XRI    #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0xef => {
        println!("RST    5");
        1
      }
      0xf0 => {
        println!("RP");
        1
      }
      0xf1 => {
        println!("POP    PSW");
        1
      }
      0xf2 => {
        println!("JP     ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xf3 => {
        println!("DI");
        1
      }
      0xf4 => {
        println!("CP     ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xf5 => {
        println!("PUSH   PSW");
        1
      }
      0xf6 => {
        println!("ORI    #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0xf7 => {
        println!("RST    6");
        1
      }
      0xf8 => {
        println!("RM");
        1
      }
      0xf9 => {
        println!("SPHL");
        1
      }
      0xfa => {
        println!("JM     ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      0xfb => {
        println!("EI");
        1
      }
      0xfc => {
        println!("CM     ${:0>2X}{:0>2X}", self.memory[pc + 2], self.memory[pc + 1]);
        3
      }
      // No 0xfd
      0xfe => {
        println!("CPI    #${:0>2X}", self.memory[pc + 1]);
        2
      }
      0xff => {
        println!("RST    7");
        1
      }
      other => {
        println!("**Missing opcode 0x{:0>2X}**", other);
        1
      }
    }
  }
}

fn get_sign(byte: u8) -> Sign {
  match byte & 0x80 {
    0 => Sign::Positive,
    _ => Sign::Negative,
  }
}

fn get_parity(byte: u8) -> Parity {
  let mut value = byte;
  value ^= value >> 4;
  value &= 0x0F;
  value = ((0x6996 >> value) & 0x01) as u8;
  if value == 0 { Parity::Even } else { Parity::Odd }
}
