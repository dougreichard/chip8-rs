use rand::prelude::*;
use std::boxed;

extern crate device_query;

use device_query::{Keycode, DeviceQuery, DeviceState};

enum Opcodes {
    Call = 0,
    JMP = 1,
    JSR = 2,
    EQU = 3,
    NEQ = 4,
}

trait Logger {
    fn log(&self, msg: &str);
}
trait Screen {
    fn draw(&self, gfx: &[u8; 64 * 32]);
}
trait Input {
    fn update_keys(&self, keys: &mut [u8; 16], last: &mut Option<u8>);
}

struct Console {}

impl Logger for Console {
    fn log(&self, msg: &str) {
        println!("{}", &msg);
    }
}
impl Console {
    fn new() -> Self {
        print!("\x1B[2J");
        Console {}
    }
}
impl Screen for Console {
    fn draw(&self, gfx: &[u8; 64 * 32]) {
        print!("\x1B[1;1H");
        for row in 0..32 {
            for col in 0..64 {
                if gfx[col as usize + row as usize * 64] == 1 {
                    print!("\x1b[0;47;1m \x1b[0m",);
                } else {
                    print!(" ");
                }
            }
            println!("");
        }
    }
}
/// Keypad                   Keyboard
// +-+-+-+-+                +-+-+-+-+
// |1|2|3|C|                |1|2|3|4|
// +-+-+-+-+                +-+-+-+-+
// |4|5|6|D|                |Q|W|E|R|
// +-+-+-+-+       =>       +-+-+-+-+
// |7|8|9|E|                |A|S|D|F|
// +-+-+-+-+                +-+-+-+-+
// |A|0|B|F|                |Z|X|C|V|
// +-+-+-+-+                +-+-+-+-+
impl Input for Console {
    fn update_keys(&self, emu_keys: &mut [u8;16], last: &mut Option<u8>) {
        let device_state = DeviceState::new();
        let keys: Vec<Keycode> = device_state.get_keys();
        *last = None;
        
        let keymap:[Keycode;16] = [
            Keycode::X,    
            Keycode::Key1,
            Keycode::Key2,
            Keycode::Key3,
            Keycode::Q,
            Keycode::W,
            Keycode::E,
            Keycode::A,
            Keycode::S,
            Keycode::D,
            Keycode::Z,
            Keycode::C,
            Keycode::Key4,
            Keycode::R,
            Keycode::F,
            Keycode::V
        ];
        for elem in emu_keys.iter_mut() { *elem = 0; }

        for key in keys.iter() {
            let pos = keymap.iter().position(|k| k==key );
            match pos {
                Some (i) => {
                    emu_keys[i] = 0xff;
                    if last.is_none() {
                        *last = Some(i as u8);
                    }
                }
                _ => {}
            }
        }
    }
}

struct Chip8 {
    opcode: u16,
    memory: [u8; 4096],
    V: [u8; 16],
    R: [u8; 16],
    I: u16,
    pc: u16,
    stack: [u16; 16],
    sp: u16,
    // hardware
    gfx: [u8; 64 * 32], // 2K 2048 pixels
    hgr: bool,
    delay_timer: u8,
    delay_start: Option<std::time::SystemTime>,
    sound_timer: u8,
    sound_start: Option<std::time::SystemTime>,
    key: [u8; 16],
    last_key: Option<u8>,
    // flags
    draw_flag: bool,
    //
    log: Box<dyn Logger>,
    screen: Box<dyn Screen>,
    input: Box<dyn Input>,
}

impl Chip8 {
    fn new(log: Box<dyn Logger>, screen: Box<dyn Screen>, input: Box<dyn Input>) -> Self {
        // Initialize registers and memory once
        Chip8 {
            opcode: 0,
            memory: [0; 4096],
            V: [0; 16],
            R: [0; 16],
            I: 0,
            pc: 0x200,
            stack: [0; 16],
            sp: 0,
            gfx: [0; 64 * 32],
            hgr: false,
            delay_timer: 0,
            sound_timer: 0,
            key: [0; 16],
            last_key: None,
            draw_flag: false,
            log,
            screen,
            input,
            delay_start: None,
            sound_start: None,
        }
    }
    fn load(&mut self, name: &str) -> bool {
        self.font();
        match std::fs::read(name) {
            Ok(buffer) => {
                for (i, b) in buffer.iter().enumerate() {
                    self.memory[i + 0x200] = *b;
                }
                true
            }
            _ => {
                self.log(&format!("file not found {}", name));
                false
            }
        }
    }
// display_clear
    fn draw_clear(&mut self) {
        self.gfx = [0; 64 * 32];
        self.pc += 2;
    }
    fn scroll_left(&mut self) {
        self.pc += 2;
    }
    fn scroll_right(&mut self) {
        self.pc += 2;
    }
    fn scroll_down(&mut self, x:u8) {
        let start = self.V[x as usize];
        for row in (start..32).rev() {
            for col in 0..64 {
                self.gfx[(row*64+col) as usize] = self.gfx[((row-start)*64+col) as usize];
            }
        }
        self.pc += 2;
    }
    fn hgr(&mut self, enable: bool)
    {
        self.hgr = enable;
        self.pc += 2;
    } 
    
    fn draw_x_y_low(&mut self, x: u8, y: u8, n: u8) {
        if self.hgr && n == 0 {
            self.draw_x_y_high(x, y, n);
            return;    
        }
        let x = self.V[x as usize];
        let y = self.V[y as usize];
        self.V[0xf] = 0;
        for row in 0..n {
            let pixel = self.memory[(self.I + row as u16) as usize];
            // bits are the columns
            for col in 0..8 {
                let bit = pixel & (0x80 >> col);
                if bit != 0 {
                    let g: usize = x as usize + col + (y as usize + row as usize) * 64;
                    if self.gfx[g] == 1 {
                        self.V[0xF] = 1;
                    }
                    self.gfx[g] ^= 1;
                }
            }
        }
        self.draw_flag = true;
        self.pc += 2;
    }
    fn draw_x_y_high(&mut self, x: u8, y: u8, n: u8) {
        let x = self.V[x as usize];
        let y = self.V[y as usize];
        self.V[0xf] = 0;
        for row in 0..0xF {
            let pixel_left = self.memory[(self.I + 2*row as u16) as usize];
            let pixel_right = self.memory[(self.I + 1 + 2*row as u16) as usize];
            let pixel = ((pixel_left as u16) << 8 ) | pixel_right as u16;

            // bits are the columns
            for col in 0..0xF {
                let bit = pixel & (0x8000 >> col);
                if bit != 0 {
                    let g: usize = x as usize + col + (y as usize + row as usize) * 64;
                    if self.gfx[g] == 1 {
                        self.V[0xF] = 1;
                    }
                    self.gfx[g] ^= 1;
                }
            }
        }
        self.draw_flag = true;
        self.pc += 2;
    }
    fn get_delay(&mut self, x: u8) {
        if let Some(time) = self.delay_start {
            if let Ok(elapsed) = time.elapsed() {
                let as_hertz:u8 = ((elapsed.as_millis() * 60) / 1000) as u8;
                print!(
                    "\x1B[3;71Hpc: Elapsed {} Delay {}",
                    &as_hertz, self.delay_timer
                );
                if as_hertz >= self.delay_timer {
                    self.V[x as usize] = 0;
                } else {
                    self.V[x as usize] = self.delay_timer - as_hertz;
                }
            }
        }
        self.pc += 2;
    }
    fn get_sound_delay(&mut self, x: u8) {
        if let Some(time) = self.sound_start {
            if let Ok(elapsed) = time.elapsed() {
                let as_hertz:u8 = ((elapsed.as_millis() * 60) / 1000) as u8;
                print!(
                    "\x1B[4;71Hpc: Elapsed {} Sound {}",
                    &as_hertz, self.sound_timer
                );
                if as_hertz > self.sound_timer {
                    self.V[x as usize] = 0;
                } else {
                    self.V[x as usize] = self.sound_timer - as_hertz;
                }
            }
        }
        self.pc += 2;
    }
    // Skip the follow instruction if VX == NN
    fn if_vx_eq_nn(&mut self, x:u8, nn:u8) {
        if self.V[x as usize] == nn {
            self.pc += 4;
        } else {
            self.pc += 2;
        }
    }
    fn if_not_eq(&mut self, x: u8, nn: u8) {
        if self.V[x as usize] != nn {
            self.pc += 4;
        } else {
            self.pc += 2;
        }
    }
    fn if_eq(&mut self, x: u8, y: u8) {
        if self.V[x as usize] == self.V[y as usize] {
            self.pc += 4;
        } else {
            self.pc += 2;
        }
    }
    fn start_delay(&mut self, x: u8) {
        self.delay_start = Some(std::time::SystemTime::now());
        self.delay_timer = self.V[x as usize];
        self.pc += 2;
    }
    fn start_sound_delay(&mut self, x: u8) {
        self.sound_start = Some(std::time::SystemTime::now());
        self.sound_timer = self.V[x as usize];
        self.pc += 2;
    }
    fn set_i(&mut self, nnn: u16) {
        self.I = nnn;
        self.pc += 2;
    }
    fn set_v(&mut self, x: u8, nn: u8) {
        self.V[x as usize] = nn;
        self.pc += 2;
    }
    fn add_v(&mut self, x: u8, nn: u8) {
        match self.V[x as usize].overflowing_add(nn) {
            (v, _) => {
                self.V[x as usize] = v;
                self.V[0xF] = 0;
            }
        }
        self.pc += 2;
    }
    fn set_v_v(&mut self, x: u8, y: u8) {
        self.V[x as usize] = self.V[y as usize];
        self.pc += 2;
    }
    // Set Vx to Vx OR Vy
    fn vx_or_vy(&mut self, x: u8, y: u8) {
        self.V[x as usize] = self.V[x as usize] | self.V[y as usize];
        self.pc += 2;
    }
    // Set Vx to Vx OR Vy
    fn vx_and_vy(&mut self, x: u8, y: u8) {
        self.V[x as usize] = self.V[x as usize] & self.V[y as usize];
        self.pc += 2;
    }
    // Set Vx to Vx OR Vy
    fn vx_xor_vy(&mut self, x: u8, y: u8) {
        self.V[x as usize] = self.V[x as usize] ^ self.V[y as usize];
        self.pc += 2;
    }
    fn vx_add_vy_carry(&mut self, x:u8, y:u8) {
        match self.V[x as usize].overflowing_add(self.V[y as usize]) {
            (v, true) => {
                self.V[x as usize] = v;
                self.V[0xF] = 1;
            }
            (v, false) => {
                self.V[x as usize] = v;
                self.V[0xF] = 0;
            }
        }
        self.pc += 2;
    }
    // Vx = Vx - Vy Vf = !borrow 
    fn vx_sub_vy_borrow(&mut self, x:u8, y:u8) {
        match self.V[x as usize].overflowing_sub(self.V[y as usize]) {
            (v, true) => {
                self.V[x as usize] = v;
                self.V[0xF] = 0;
            }
            (v, false) => {
                self.V[x as usize] = v;
                self.V[0xF] = 1;
            }
        }
        self.pc += 2;
    }
    // Vx = Vy - Vx Vf = !borrow 
    fn vy_sub_vx_borrow(&mut self, x:u8, y:u8) {
        match self.V[y as usize].overflowing_sub(self.V[x as usize]) {
            (v, true) => {
                self.V[x as usize] = v;
                self.V[0xF] = 0;
            }
            (v, false) => {
                self.V[x as usize] = v;
                self.V[0xF] = 1;
            }
        }
        self.pc += 2;
    }
    // Shift Vy one right and store it in Vx Vf is the shifted bit
    fn vx_as_rshift_vy(&mut self, x:u8, y:u8) {
        self.V[0xF] = self.V[y as usize] & 0x1;
        self.V[x as usize] = self.V[y as usize] >> 1;
        self.pc += 2;
    }
    // Shift Vy left one and store it in Vx Vf is the shifted bit
    fn vx_as_lshift_vy(&mut self, x:u8, y:u8)
    {
        self.V[0xF] = if self.V[y as usize] & 0xF0 != 0 { 1 } else { 0 };
        self.V[x as usize] = self.V[y as usize] << 1;
        self.pc += 2;
    }

    fn if_vx_eq_vy(&mut self, x:u8, y:u8) {
        // If V[x] == V[y]
        if self.V[x as usize] != self.V[y as usize] {
            self.pc += 4;
        } else {
            self.pc += 2;
        }
    }
    fn i_add_vx(&mut self, x: u8) {
        self.I += self.V[x as usize] as u16;
        self.pc += 2;
    }
    fn jmp(&mut self, nnn: u16) {
        self.pc = nnn;
    }
    // JUMP to V0 + nnn
    fn jmp_v0(&mut self, nnn:u16) {
        self.pc = self.V[0] as u16 + nnn;
    }
    fn jsr(&mut self, nnn:u16) {
        self.stack[self.sp as usize] = self.pc;
        self.pc = nnn;
        self.sp += 1;
    }
    fn ret(&mut self) {
        self.pc = self.stack[(self.sp - 1) as usize];
        self.sp -= 1;
        // Returned to last PC, need to advance
        self.pc += 2;
    } 
    fn exit(&mut self) {
        self.pc = 0xFFFF;
    }
    fn native_call(&mut self, nnn: u16) {
        self.log("Machine language??");
    }
    fn vx_rnd(&mut self, x:u8, nn: u8) {
        self.V[x as usize] = random::<u8>() & nn;
        self.pc += 2;
    }
    fn i_as_sprite_vx(&mut self, x:u8) {
        self.I = 0x50 + (5 * self.V[x as usize]) as u16;
        self.pc += 2;
    }
    fn i_as_hgr_sprite_vx(&mut self, x:u8) {
        self.I = 0xA0 + (10 * self.V[x as usize]) as u16;
        self.pc += 2;
    }

    fn vx_as_bcd(&mut self, x: u8) {
        let I = self.I as usize;
        let x = x as usize;
        self.memory[I] = (self.V[x] / 100) as u8;
        self.memory[I + 1] = (self.V[x] / 10) % 10 as u8;
        self.memory[I + 2] = (self.V[x] % 10) as u8;
        self.pc += 2;
    }

    fn store_v0_vx(&mut self, x: u8) {
        let count = x as usize;
        for c in 0..=count {
            self.memory[self.I as usize + c] = self.V[c];
        }
        self.I += count as u16 + 1;
        self.pc += 2;
    }
    fn read_v0_vx(&mut self, x:u8) {
        let count = x as usize;
        for c in 0..=count {
            self.V[c] = self.memory[self.I as usize + c];
        }
        self.I += count as u16 + 1;
        self.pc += 2;
    }
    fn store_rpl_v0_vx(&mut self, x: u8){
        let count = x as usize;
        for c in 0..=count {
            self.R[self.I as usize + c] = self.V[c];
        }
        self.I += count as u16 + 1;
        self.pc += 2;
    }
    fn read_rpl_v0_vx(&mut self, x:u8){
        let count = x as usize;
        for c in 0..=count {
            self.V[c] = self.R[self.I as usize + c];
        }
        self.I += count as u16 + 1;
        self.pc += 2;
    }
    fn skip_if_key_vx(&mut self, x: u8) {
        let key = self.V[x as usize];
        if self.key[key as usize] != 0 {
            self.pc+=4;
        } else {
            self.pc += 2;
        }
    }
    fn skip_if_not_key_vx(&mut self, x: u8) {
        let key = self.V[x as usize];
        if self.key[key as usize] == 0 {
            self.pc+=4;
        } else {
            self.pc += 2;
        }
    }
    fn wait_for_next_key(&mut self, x: u8) {
        // TODO: KET PRESS
        //
        if let Some(key) = self.last_key {
            self.V[x as usize] = key;
            self.pc += 2;
        }
        
    }





    fn emulate_cycle(&mut self) -> bool {
        // fetch opcode
        let b0 = self.memory[(self.pc) as usize];
        let b1 = self.memory[(self.pc + 1) as usize];
        let n0 = b0 >> 4;
        let n1 = b0 & 0x0F;
        let n2 = b1 >> 4;
        let n3 = b1 & 0x0F;
        let n = n3;
        let nn = b1;
        let nnn: u16 = (n1 as u16) << 8 | nn as u16;
        //let pre_pc = self.pc;
        print!("\x1B[1;71Hpc: {} {}:{}:{}:{}", self.pc, n0, n1, n2, n3);
        // decode Opcode
        // Match based on the 4 bytes
        match (n0, n1, n2, n3) {
            (0, 0, 0xC, x) => self.scroll_down(x),
            (0, 0, 0xF, 0xB) => self.scroll_right(),
            (0, 0, 0xF, 0xC) => self.scroll_left(),
            (0, 0, 0xF, 0xD) => self.exit(),
            (0, 0, 0xF, 0xE) => self.hgr(false),
            (0, 0, 0xF, 0xF) => self.hgr(true),
            (0, 0, 0xE, 0) => self.draw_clear(),
            (0, 0, 0xE, 0xE) => self.ret(),
            (0, _, _, _) => self.native_call(nnn),
            (1, _, _, _) => self.jmp(nnn),
            (2, _, _, _) => self.jsr(nnn),
            (3, x, _, _) => self.if_vx_eq_nn(x,nn),
            (4, x, _, _) => self.if_not_eq(x, nn),
            (5, x, y, 0) => self.if_eq(x, y),
            (6, x, _, _) => self.set_v(x, nn),
            (7, x, _, _) => self.add_v(x, nn),
            (8, x, y, 0) => self.set_v_v(x, y),
            (8, x, y, 1) => self.vx_or_vy(x, y),
            (8, x, y, 2) => self.vx_and_vy(x, y),
            (8, x, y, 3) => self.vx_xor_vy(x, y),
            (8, x, y, 4) => self.vx_add_vy_carry(x,y),
            (8, x, y, 5) => self.vx_sub_vy_borrow(x,y),
            (8, x, y, 6) => self.vx_as_rshift_vy(x,y),
            (8, x, y, 7) => self.vy_sub_vx_borrow(x,y),
            (8, x, y, 0xE) => self.vx_as_lshift_vy(x,y),
            (9, x, y, 0) => self.if_vx_eq_vy(x,y),
            (0xA, _, _, _) => self.set_i(nnn),
            (0xB, _, _, _) => self.jmp_v0(nnn),
            (0xC, x, _, _) => self.vx_rnd(x,nn),
            (0xD, x, y, n) => self.draw_x_y_low(x, y, n),
            (0xE, x, 9, 0xE) => self.skip_if_key_vx(x),
            (0xE, x, 0xA, 1) => self.skip_if_not_key_vx(x),
            (0xF, x, 0, 7) => self.get_delay(x),
            (0xF, x, 0, 0xA) => self.wait_for_next_key(x),
            (0xF, x, 1, 5) => self.start_delay(x),
            (0xF, x, 1, 8) => self.start_sound_delay(x),
            (0xF, x, 1, 0xE) => self.i_add_vx(x),
            (0xF, x, 2, 9) => self.i_as_sprite_vx(x),
            (0xF, x, 3, 0) => self.i_as_hgr_sprite_vx(x),
            (0xF, x, 3, 3) => self.vx_as_bcd(x),
            (0xF, x, 5, 5) => self.store_v0_vx(x),
            (0xF, x, 6, 5) => self.read_v0_vx(x),
            (0xF, x, 7, 5) => self.store_rpl_v0_vx(x),
            (0xF, x, 8, 5) => self.read_rpl_v0_vx(x),
            _ => {
                self.log("Unknown Opcode");
            }
        }
        true
    }
    fn set_keys(&mut self) {}

    fn run_tick(&mut self) -> bool {
        let ret = self.emulate_cycle();
        ret
    }

    fn run(&mut self) {
        let clock = std::time::SystemTime::now();
        loop {
            self.input.update_keys(&mut self.key, &mut self.last_key);
            match clock.elapsed() {
                Ok(elapsed) => {
                    let as_hertz = (elapsed.as_millis() * 550) / 1000;
                    if as_hertz >= 550 {
                        self.run_tick();
                        if self.draw_flag {
                            self.screen.draw(&self.gfx);
                            self.draw_flag = false;
                        }
                    }
                }
                Err(_) => {}
            }
        }
        // one last screen draw
        self.screen.draw(&self.gfx);
    }

    fn log(&self, msg: &str) {
        self.log.log(msg);
    }
    fn font(&mut self) {
        let font = [
        ///////////////////////////////
        /// 0x50 start of low res
            0xf0, 0x90, 0x90, 0x90, 0xf0, // 0
            0x20, 0x60, 0x20, 0x20, 0x70, // 1
            0xF0, 0x10, 0xF0, 0x80, 0xF0, // 2
            0xF0, 0x10, 0xF0, 0x10, 0xF0, // 3
            0x90, 0x90, 0xF0, 0x10, 0x10, // 4
            0xf0, 0x80, 0xf0, 0x10, 0xf0, // 5
            0xf0, 0x80, 0xf0, 0x90, 0xf0, // 6
            0xf0, 0x10, 0x20, 0x40, 0x40, // 7
            0xf0, 0x90, 0xf0, 0x90, 0xf0, // 8
            0xf0, 0x90, 0xf0, 0x10, 0xf0, // 9
            0xF0, 0x90, 0xF0, 0x90, 0x90, // A
            0xE0, 0x90, 0xE0, 0x90, 0xE0, // B
            0xf0, 0x80, 0x80, 0x80, 0xf0, // C
            0xf0, 0x90, 0x90, 0x90, 0xf0, // D
            0xf0, 0x80, 0xe0, 0x80, 0xf0, // E
            0xf0, 0x80, 0xf0, 0x80, 0x80, // F
        ///////////////////////////////////////////////////
        /// 0xD0 start of high res
        ////////////////////////////////////////////////////////////////////////
        0x3C, 0x7E, 0xE7, 0xC3, 0xC3, 0xC3, 0xC3, 0xE7, 0x7E, 0x3C, 
        0x18, 0x38, 0x58, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C,
        0x3E, 0x7F, 0xC3, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xFF, 0xFF,
        0x3C, 0x7E, 0xC3, 0x03, 0x0E, 0x0E, 0x03, 0xC3, 0x7E, 0x3C,
        0x06, 0x0E, 0x1E, 0x36, 0x66, 0xC6, 0xFF, 0xFF, 0x06, 0x06,
        0xFF, 0xFF, 0xC0, 0xC0, 0xFC, 0xFE, 0x03, 0xC3, 0x7E, 0x3C,
        0x3E, 0x7C, 0xC0, 0xC0, 0xFC, 0xFE, 0xC3, 0xC3, 0x7E, 0x3C,
        0xFF, 0xFF, 0x03, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x60, 0x60,
        0x3C, 0x7E, 0xC3, 0xC3, 0x7E, 0x7E, 0xC3, 0xC3, 0x7E, 0x3C,
        0x3C, 0x7E, 0xC3, 0xC3, 0x7F, 0x3F, 0x03, 0x03, 0x3E, 0x7C];
        for (i, b) in font.iter().enumerate() {
            self.memory[0x50 + i] = *b;
        }
    }
}

//
// https://multigesture.net/articles/how-to-write-an-emulator-chip-8-interpreter/
//
// Memory Map
// 0x000 - 01FF - Chip * interpreter (contains font set in emu)
// 0x050 - 0x0A0 - Used for the built in 4x5 pixel font set (0-F)
// 0x200- 0xFFF - Program ROM and RAM

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let file = if args.len() == 2 {
        &args[1]
    } else {
        "./rom/test_opcode.ch8"
    };
    println!("{}", 137 % 10);
    let all = Box::new(Console {});
    let screen = Box::new(Console::new());
    let input = Box::new(Console::new());
    let mut emu = Chip8::new(all, screen, input);
    if emu.load(file) {
        emu.run();
    }
}
