// build command:
// cargo +nightly build --release

#![feature(decl_macro,proc_macro_hygiene)]

#[macro_use] extern crate rocket;

use std::io::prelude::*;
use std::sync::mpsc::{sync_channel, SyncSender, Receiver};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

use rand::Rng;
use rocket::request::Form;
use rocket::State;
use spidev::{Spidev, SpidevOptions, SpiModeFlags};

struct LedSender {
    sender: SyncSender<LedState>,
}

struct LedState {
    color1: ColorHsv,
    color2: ColorHsv,
    color3: ColorHsv,
    pattern: u8,
}

#[derive(FromForm)]
struct LedStateData {
    color1_h: f32,
    color1_s: f32,
    color1_v: f32,
    color2_h: f32,
    color2_s: f32,
    color2_v: f32,
    color3_h: f32,
    color3_s: f32,
    color3_v: f32,
    pattern: u8,
}

struct Pixel {
    r: u8,
    g: u8,
    b: u8,
}

struct ColorHsv {
    h: f32,
    s: f32,
    v: f32,
}

impl ColorHsv {
    fn set_brightness(&mut self, brightness: f32) {
        self.v = (self.v * brightness.max(0.0)).min(1.0);
    }
}

struct Sprite {
    pos: f32,
    falloff: f32,
}

struct SpriteEnvelope {
    pos: f32,
    falloff: f32,
    live: bool,
    state: SpriteState,
    attack: f32,
    sustain: f32,
    release: f32,
    timer: f32,
    level: f32,
}

impl SpriteEnvelope {
    fn new() -> SpriteEnvelope {
        SpriteEnvelope { 
            pos: 0.0, 
            falloff: 1.0, 
            live: true, 
            state: SpriteState::Attack,
            attack: 1000.0,
            sustain: 1000.0,
            release: 1000.0,
            timer: 0.0,
            level: 0.0,
        }
    }
    
    fn run(&mut self, dt: f32) {
        if self.live {
            //~ self.timer = (self.timer - dt).max(0.0);
            //~ if self.timer <= 0.0 {
                //~ self.live = false;
            //~ }
            match self.state {
                SpriteState::Attack => {
                    self.timer = self.timer + dt;
                    
                    //~ self.level = (dt * 0.005 + self.level).min(1.0);
                    self.level = (dt / self.attack + self.level).min(1.0);
                    
                    if self.level >= 1.0 {
                        self.state = SpriteState::Sustain;
                        self.timer = 0.0;
                    }
                }
                SpriteState::Sustain => {
                    self.timer = self.timer + dt;
                    
                    if self.timer >= self.sustain {
                        self.state = SpriteState::Release;
                        self.timer = 0.0;
                    }
                }
                SpriteState::Release => {
                    self.timer = self.timer + dt;
                    
                    //~ self.level = (self.level - (dt * 0.005)).max(0.0);
                    self.level = (self.level - (dt / self.release)).min(1.0);
                    
                    if self.level <= 0.0 {
                        self.live = false;
                    }
                }
            }
        }
    }
    
    fn reset(&mut self) {
        self.live = true;
        self.state = SpriteState::Attack;
        self.timer = 0.0;
        self.level = 0.0;
    }
}

enum SpriteState {
    Attack,
    Sustain,
    Release,
}

fn write_leds(spi: &mut Spidev, leds: &Vec<Pixel>, buffer: &mut Vec<u8>, offset: usize) {
    //let mut tmp = Vec::new();
    //for _i in 0..4 {
    //    tmp.push(0);
    //}
    buffer.clear();
    for _i in 0..4 {
        buffer.push(0);
    }
    //~ for led in leds.iter() {
        //~ tmp.push(255);
        //~ tmp.push(led.b);
        //~ tmp.push(led.g);
        //~ tmp.push(led.r);
    //~ }
    
    for i in offset..(offset + leds.len()) {
        let i_wrap = i % leds.len();
        //tmp.push(255);
        //tmp.push(leds[i_wrap].b);
        //tmp.push(leds[i_wrap].g);
        //tmp.push(leds[i_wrap].r);
        buffer.push(255);
        buffer.push(leds[i_wrap].b);
        buffer.push(leds[i_wrap].g);
        buffer.push(leds[i_wrap].r);
    }
    
    for _i in 0..8 {
        //tmp.push(0);
        buffer.push(0);
    }
    //spi.write(&tmp[..]).unwrap();
    spi.write(&buffer[..]).unwrap();
}

fn hsv_2_rgb(col: &ColorHsv) -> Pixel {
    let h_wrap = col.h.rem_euclid(1.0);
    let mut out = Pixel { r: 0, g: 0, b: 0 };
    match (h_wrap * 6.0).trunc() as u8 {
        0 => {
            out.r = (col.v * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s * (1.0 - ((col.h * 6.0) - ((col.h * 6.0).trunc()))))) * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s)) * 255.0) as u8;
        }
        1 => {
            out.r = ((col.v * (1.0 - col.s * ((col.h * 6.0) - ((col.h * 6.0).trunc())))) * 255.0) as u8;
            out.g = (col.v * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s)) * 255.0) as u8;
        }
        2 => {
            out.r = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.g = (col.v * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s * (1.0 - ((col.h * 6.0) - ((col.h * 6.0).trunc()))))) * 255.0) as u8;
        }
        3 => {
            out.r = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s * ((col.h * 6.0) - ((col.h * 6.0).trunc())))) * 255.0) as u8;
            out.b = (col.v * 255.0) as u8;
        }
        4 => {
            out.r = ((col.v * (1.0 - col.s * (1.0 - ((col.h * 6.0) - ((col.h * 6.0).trunc()))))) * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.b = (col.v * 255.0) as u8;
        }
        5 => {
            out.r = (col.v * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s * ((col.h * 6.0) - ((col.h * 6.0).trunc())))) * 255.0) as u8;
        }
        _ => (),
    }
    out
}

fn hsv_interp(col1: &ColorHsv, col2: &ColorHsv, pos: f32) -> ColorHsv {
    let h_range = col1.h - col2.h;
    let s_range = col1.s - col2.s;
    let v_range = col1.v - col2.v;
    
    let h_out = col1.h - (h_range * pos.min(1.0));
    let s_out = col1.s - (s_range * pos.min(1.0));
    let v_out = col1.v - (v_range * pos.min(1.0));
    
    ColorHsv { h: h_out, s: s_out, v: v_out }
}

fn hsv_interp_3(col1: &ColorHsv, col2: &ColorHsv, col3: &ColorHsv, pos: f32) -> ColorHsv {
    if pos > 0.0 {
        let h_range = col1.h - col2.h;
        let s_range = col1.s - col2.s;
        let v_range = col1.v - col2.v;
        
        let h_out = col1.h - (h_range * pos.min(1.0));
        let s_out = col1.s - (s_range * pos.min(1.0));
        let v_out = col1.v - (v_range * pos.min(1.0));

        ColorHsv { h: h_out, s: s_out, v: v_out }
    } else if pos < 0.0 {
        let h_range = col1.h - col3.h;
        let s_range = col1.s - col3.s;
        let v_range = col1.v - col3.v;
        
        let h_out = col1.h - (h_range * (pos * -1.0).min(1.0));
        let s_out = col1.s - (s_range * (pos * -1.0).min(1.0));
        let v_out = col1.v - (v_range * (pos * -1.0).min(1.0));

        ColorHsv { h: h_out, s: s_out, v: v_out }
    } else {
        let h_out = col1.h;
        let s_out = col1.s;
        let v_out = col1.v;

        ColorHsv { h: h_out, s: s_out, v: v_out }
    }
}

fn triangle(pos: f32, phs: f32) -> f32 {
    ((pos + phs).rem_euclid(1.0) * 2.0 - 1.0).abs() * 2.0 - 1.0
}

#[get("/")]
fn index() -> &'static str {
    "foo bar"
}

/*
#[get("/off")]
fn off(led_sender: State<LedSender>) {
    //~ led_sender.sender.send(LedState {
        //~ color1: Pixel { r: 63, g: 0, b: 0 },
        //~ color2: Pixel { r: 0, g: 63, b: 0 },
        //~ color3: Pixel { r: 0, g: 0, b: 63 },
        //~ pattern: 0 }).unwrap();
}
*/

#[post("/set", data = "<data>")]
fn set(led_sender: State<LedSender>, data: Form<LedStateData>) {
    led_sender.sender.send(LedState {
        color1: ColorHsv { h: data.color1_h, s: data.color1_s, v: data.color1_v },
        color2: ColorHsv { h: data.color2_h, s: data.color2_s, v: data.color2_v },
        color3: ColorHsv { h: data.color3_h, s: data.color3_s, v: data.color3_v },
        pattern: data.pattern }).unwrap();
}

fn main() {
    let (tx, rx): (SyncSender<LedState>, Receiver<LedState>) = sync_channel(1);
    thread::spawn(move || {
        let mut spidev = Spidev::open("/dev/spidev0.0").unwrap();
        let options = SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(8_000_000)
            .mode(SpiModeFlags::SPI_MODE_0)
            .build();
        spidev.configure(&options).unwrap();

        let led_count: usize = 100;
        let sprites_random_count: usize = 20;
        
        let mut leds_1 = Vec::with_capacity(led_count);
        for _i in 0..led_count {
            leds_1.push(Pixel { r: 0, g: 0, b: 0 });
        }

        let mut leds_off = Vec::with_capacity(led_count);
        for _i in 0..led_count {
            leds_off.push(Pixel { r: 0, g: 0, b: 0 });
        }

        let mut buffer = Vec::with_capacity(1024);
        
        let mut offset: f32 = 0.0;
        let offset_inc = 0.0005;
        //~ let mut offset_count: u8 = 0;
        //~ let offset_frames: u8 = 5;
        
        let loop_time: f32 = 16.6666;
        let mut pattern: u8 = 0;
        let max_brightness: f32 = 0.5;
        
        let mut rng = rand::thread_rng();
        
        let mut color1 = ColorHsv { h: 0.0, s: 0.0, v: 0.0 };
        let mut color2 = ColorHsv { h: 0.0, s: 0.0, v: 0.0 };
        let mut color3 = ColorHsv { h: 0.0, s: 0.0, v: 0.0 };
        
        let mut scanner = Sprite { pos: 0.0, falloff: 10.0 };
        let mut sprite_random = SpriteEnvelope::new();
        sprite_random.falloff = 20.0;
        sprite_random.attack = 500.0;
        sprite_random.sustain = 250.0;
        sprite_random.release = 500.0;
        
        let mut sprites_random = Vec::with_capacity(sprites_random_count);
        for _i in 0..sprites_random_count {
            let mut sprite = SpriteEnvelope::new();
            sprite.pos = rng.gen();
            sprite.falloff = 100.0;
            sprite.attack = rng.gen::<f32>() * 500.0 + 500.0;
            sprite.sustain = 500.0;
            sprite.release = rng.gen::<f32>() * 500.0 + 500.0;
            sprites_random.push(sprite);
        }

        loop {
            let loop_start = Instant::now();
            let msg = rx.try_recv();

            match msg {
                Ok(msg) => {
                    match msg.pattern {
                        0 => {
                            write_leds(&mut spidev, &leds_off, &mut buffer, 0);
                            pattern = msg.pattern;
                        }
                        1 => {
                            color1 = msg.color1;
                            color2 = msg.color2;
                            color3 = msg.color3;
                            pattern = msg.pattern;
                            
                            color1.set_brightness(max_brightness);
                            color2.set_brightness(max_brightness);
                            color3.set_brightness(max_brightness);
                                
                            //~ for (i, led) in leds_1.iter_mut().enumerate() {
                                //~ let pos = (i as f32) / ((led_count - 1) as f32);
                                //~ *led = hsv_2_rgb(&hsv_interp_3(&color1, &color2, &color3, triangle(0.0 + offset, pos)));
                            //~ }
                            
                            //~ write_leds(&mut spidev, &leds_1, 0);
                        }
                        2 => {
                            color1 = msg.color1;
                            color2 = msg.color2;
                            color3 = msg.color3;
                            pattern = msg.pattern;
                            
                            color1.set_brightness(max_brightness);
                            color2.set_brightness(max_brightness);
                            color3.set_brightness(max_brightness);
                                
                            //~ scanner.pos = (triangle(offset, 0.0) + 1.0) * 0.5;
                    
                            //~ for (i, led) in leds_1.iter_mut().enumerate() {
                                //~ let pos = (i as f32) / ((led_count - 1) as f32);
                                //~ let delta = (pos - scanner.pos).abs();
                                //~ let mix = (delta + (delta * scanner.falloff)).min(1.0);
                                
                                //~ let col_interp = hsv_interp(&color1, &color2, pos);
                                
                                //~ *led = hsv_2_rgb(&hsv_interp(&color3, &col_interp, mix));
                            //~ }
                            
                            //~ write_leds(&mut spidev, &leds_1, 0);
                        }
                        3 => {
                            color1 = msg.color1;
                            color2 = msg.color2;
                            color3 = msg.color3;
                            pattern = msg.pattern;
                            
                            color1.set_brightness(max_brightness);
                            color2.set_brightness(max_brightness);
                            color3.set_brightness(max_brightness);
                            
                            sprite_random.pos = rng.gen();
                            for i in sprites_random.iter_mut() {
                                i.pos = rng.gen();
                            }
                            
                            //~ for (i, led) in leds_1.iter_mut().enumerate() {
                                //~ let pos = (i as f32) / ((led_count - 1) as f32);
                                //~ let delta = (pos - sprite_random.pos).abs();
                                //~ let mix = (delta + (delta * sprite_random.falloff)).min(1.0);
                                
                                //~ let col_interp = hsv_interp(&color1, &color2, pos);
                                
                                //~ *led = hsv_2_rgb(&hsv_interp(&color3, &col_interp, mix + (1.0 - sprite_random.level)));
                            //~ }
                            
                            //~ write_leds(&mut spidev, &leds_1, 0);
                        }
                        _ => (),
                    }
                }
                Err(_) => (),
            }
            
            match pattern {
                0 => (),
                1 => {
                    for (i, led) in leds_1.iter_mut().enumerate() {
                        let pos = (i as f32) / ((led_count - 1) as f32);
                        *led = hsv_2_rgb(&hsv_interp_3(&color1, &color2, &color3, triangle(0.0 + offset, pos)));
                    }
                    write_leds(&mut spidev, &leds_1, &mut buffer, 0);
                }
                2 => {
                    scanner.pos = (triangle(offset, 0.0) + 1.0) * 0.5;
                    
                    for (i, led) in leds_1.iter_mut().enumerate() {
                        let pos = (i as f32) / ((led_count - 1) as f32);
                        let delta = (pos - scanner.pos).abs();
                        let mix = (delta + (delta * scanner.falloff)).min(1.0);
                        
                        let col_interp = hsv_interp(&color1, &color2, pos);
                        
                        *led = hsv_2_rgb(&hsv_interp(&color3, &col_interp, mix));
                    }
                    
                    write_leds(&mut spidev, &leds_1, &mut buffer, 0);
                }
                3 => {
                    /*
                    if sprite_random.live == false {
                        sprite_random.pos = rng.gen();
                        sprite_random.reset();
                        //~ sprite_random.live = true;
                        //~ sprite_random.state = SpriteState::Attack;
                        //~ sprite_random.timer = 0.0;
                        //~ sprite_random.level = 0.0;
                    }
                    sprite_random.run(loop_time);
                    */
                    
                    for i in sprites_random.iter_mut() {
                        if i.live == false {
                            i.pos = rng.gen();
                            i.reset();
                            i.attack = rng.gen::<f32>() * 500.0 + 500.0;
                            //~ sprite.sustain = 250.0;
                            i.release = rng.gen::<f32>() * 500.0 + 500.0;
                        }
                        i.run(loop_time);
                    }
                    
                    //~ for (i, led) in leds_1.iter_mut().enumerate() {
                        //~ let pos = (i as f32) / ((led_count - 1) as f32);
                        //~ let delta = (pos - sprite_random.pos).abs();
                        //~ let mix = (delta * sprite_random.falloff).min(1.0);
                        
                        //~ let col_interp = hsv_interp(&color1, &color2, pos);
                        
                        //~ *led = hsv_2_rgb(&hsv_interp(&color3, &col_interp, mix + (1.0 - sprite_random.level)));
                    //~ }
                    
                    for (i, led) in leds_1.iter_mut().enumerate() {
                        let pos = (i as f32) / ((led_count - 1) as f32);
                        let col_interp = hsv_interp(&color1, &color2, pos);
                        let mut mix_total = 0.0;
                        
                        for i in sprites_random.iter() {
                            let delta = (pos - i.pos).abs();
                            let mix = (i.level - (delta * i.falloff)).max(0.0);
                            mix_total = (mix_total + mix).min(1.0);
                        }
                        
                        *led = hsv_2_rgb(&hsv_interp(&col_interp, &color3, mix_total));
                    }
                    
                    write_leds(&mut spidev, &leds_1, &mut buffer, 0);
                }
                _ => (),
            }
            
            //~ offset_count += 1;
            //~ if offset_count >= (offset_frames - 1) {
                //~ offset_count = 0;
                //~ offset += 1;
                //~ if offset > (leds_1.len() - 1) {
                    //~ offset = 0;
                //~ }
            //~ }
            
            offset = (offset + offset_inc).rem_euclid(1.0);
            
            let loop_time_adj = (loop_time - (loop_start.elapsed().as_millis() as f32)) / 1000.0;
            if (loop_start.elapsed().as_millis() as f32) < loop_time {
                sleep(Duration::from_secs_f32(loop_time_adj));
            }
            //~ sleep(Duration::from_secs_f32(loop_time / 1000.0));
        }
    });

    rocket::ignite().manage(LedSender { sender: tx }).mount("/", routes![index, set]).launch();
}
