//! # 2D Graphics Primitives
//!
//! Provides basic 2D drawing operations: lines, rectangles, circles, etc.

use super::{color::Color, framebuffer};

/// Draws a horizontal line
pub fn hline(x1: u32, x2: u32, y: u32, color: Color) {
    let (start, end) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
    if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
        for x in start..=end {
            fb.set_pixel(x, y, color);
        }
    }
}

/// Draws a vertical line
pub fn vline(x: u32, y1: u32, y2: u32, color: Color) {
    let (start, end) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
    if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
        for y in start..=end {
            fb.set_pixel(x, y, color);
        }
    }
}

/// Draws a line using Bresenham's algorithm
pub fn line(x1: i32, y1: i32, x2: i32, y2: i32, color: Color) {
    let dx = (x2 - x1).abs();
    let dy = -(y2 - y1).abs();
    let sx = if x1 < x2 { 1 } else { -1 };
    let sy = if y1 < y2 { 1 } else { -1 };
    let mut err = dx + dy;
    
    let mut x = x1;
    let mut y = y1;
    
    if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
        loop {
            if x >= 0 && y >= 0 {
                fb.set_pixel(x as u32, y as u32, color);
            }
            
            if x == x2 && y == y2 {
                break;
            }
            
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }
}

/// Draws a rectangle outline
pub fn rect(x: u32, y: u32, width: u32, height: u32, color: Color) {
    if width == 0 || height == 0 {
        return;
    }
    
    let x2 = x + width - 1;
    let y2 = y + height - 1;
    
    hline(x, x2, y, color);
    hline(x, x2, y2, color);
    vline(x, y, y2, color);
    vline(x2, y, y2, color);
}

/// Draws a filled rectangle
pub fn fill_rect(x: u32, y: u32, width: u32, height: u32, color: Color) {
    framebuffer::fill_rect(x, y, width, height, color);
}

/// Draws a rectangle with rounded corners
pub fn rounded_rect(x: u32, y: u32, width: u32, height: u32, radius: u32, color: Color) {
    if width < 2 * radius || height < 2 * radius {
        return rect(x, y, width, height, color);
    }
    
    let x2 = x + width - 1;
    let y2 = y + height - 1;
    
    // Top and bottom edges
    hline(x + radius, x2 - radius, y, color);
    hline(x + radius, x2 - radius, y2, color);
    
    // Left and right edges
    vline(x, y + radius, y2 - radius, color);
    vline(x2, y + radius, y2 - radius, color);
    
    // Corners
    corner_arc(x + radius, y + radius, radius, Corner::TopLeft, color);
    corner_arc(x2 - radius, y + radius, radius, Corner::TopRight, color);
    corner_arc(x + radius, y2 - radius, radius, Corner::BottomLeft, color);
    corner_arc(x2 - radius, y2 - radius, radius, Corner::BottomRight, color);
}

/// Corner positions for rounded rectangles
#[derive(Clone, Copy)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Draws a quarter-circle corner arc
fn corner_arc(cx: u32, cy: u32, radius: u32, corner: Corner, color: Color) {
    let mut x = 0i32;
    let mut y = radius as i32;
    let mut d = 3 - 2 * radius as i32;
    
    let draw_point = |px: i32, py: i32| {
        if px >= 0 && py >= 0 {
            if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
                fb.set_pixel(px as u32, py as u32, color);
            }
        }
    };
    
    while x <= y {
        let (px, py) = match corner {
            Corner::TopLeft => (cx as i32 - x, cy as i32 - y),
            Corner::TopRight => (cx as i32 + x, cy as i32 - y),
            Corner::BottomLeft => (cx as i32 - x, cy as i32 + y),
            Corner::BottomRight => (cx as i32 + x, cy as i32 + y),
        };
        draw_point(px, py);
        
        let (px, py) = match corner {
            Corner::TopLeft => (cx as i32 - y, cy as i32 - x),
            Corner::TopRight => (cx as i32 + y, cy as i32 - x),
            Corner::BottomLeft => (cx as i32 - y, cy as i32 + x),
            Corner::BottomRight => (cx as i32 + y, cy as i32 + x),
        };
        draw_point(px, py);
        
        x += 1;
        if d > 0 {
            y -= 1;
            d = d + 4 * (x - y) + 10;
        } else {
            d = d + 4 * x + 6;
        }
    }
}

/// Draws a circle outline using midpoint algorithm
pub fn circle(cx: u32, cy: u32, radius: u32, color: Color) {
    let mut x = 0i32;
    let mut y = radius as i32;
    let mut d = 3 - 2 * radius as i32;
    
    let draw_symmetric = |x: i32, y: i32| {
        let points = [
            (cx as i32 + x, cy as i32 + y),
            (cx as i32 - x, cy as i32 + y),
            (cx as i32 + x, cy as i32 - y),
            (cx as i32 - x, cy as i32 - y),
            (cx as i32 + y, cy as i32 + x),
            (cx as i32 - y, cy as i32 + x),
            (cx as i32 + y, cy as i32 - x),
            (cx as i32 - y, cy as i32 - x),
        ];
        
        if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
            for (px, py) in points {
                if px >= 0 && py >= 0 {
                    fb.set_pixel(px as u32, py as u32, color);
                }
            }
        }
    };
    
    while x <= y {
        draw_symmetric(x, y);
        x += 1;
        if d > 0 {
            y -= 1;
            d = d + 4 * (x - y) + 10;
        } else {
            d = d + 4 * x + 6;
        }
    }
}

/// Draws a filled circle
pub fn fill_circle(cx: u32, cy: u32, radius: u32, color: Color) {
    let mut x = 0i32;
    let mut y = radius as i32;
    let mut d = 3 - 2 * radius as i32;
    
    let draw_hline_symmetric = |x: i32, y: i32| {
        let y_pos = cy as i32 + y;
        let y_neg = cy as i32 - y;
        let x1 = (cx as i32 - x).max(0) as u32;
        let x2 = (cx as i32 + x).max(0) as u32;
        
        if y_pos >= 0 {
            hline(x1, x2, y_pos as u32, color);
        }
        if y_neg >= 0 && y_neg != y_pos {
            hline(x1, x2, y_neg as u32, color);
        }
    };
    
    while x <= y {
        draw_hline_symmetric(x, y);
        draw_hline_symmetric(y, x);
        
        x += 1;
        if d > 0 {
            y -= 1;
            d = d + 4 * (x - y) + 10;
        } else {
            d = d + 4 * x + 6;
        }
    }
}

/// Draws an ellipse outline
pub fn ellipse(cx: u32, cy: u32, rx: u32, ry: u32, color: Color) {
    let rx2 = (rx * rx) as i64;
    let ry2 = (ry * ry) as i64;
    let two_rx2 = 2 * rx2;
    let two_ry2 = 2 * ry2;
    
    let mut x = 0i32;
    let mut y = ry as i32;
    let mut px = 0i64;
    let mut py = two_rx2 * y as i64;
    
    let draw_symmetric = |x: i32, y: i32| {
        let points = [
            (cx as i32 + x, cy as i32 + y),
            (cx as i32 - x, cy as i32 + y),
            (cx as i32 + x, cy as i32 - y),
            (cx as i32 - x, cy as i32 - y),
        ];
        
        if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
            for (px, py) in points {
                if px >= 0 && py >= 0 {
                    fb.set_pixel(px as u32, py as u32, color);
                }
            }
        }
    };
    
    // Region 1
    let mut p = ry2 - (rx2 * ry as i64) + (rx2 / 4);
    while px < py {
        draw_symmetric(x, y);
        x += 1;
        px += two_ry2;
        if p < 0 {
            p += ry2 + px;
        } else {
            y -= 1;
            py -= two_rx2;
            p += ry2 + px - py;
        }
    }
    
    // Region 2
    p = ry2 * (x as i64 * 2 + 1) * (x as i64 * 2 + 1) / 4 
        + rx2 * (y as i64 - 1) * (y as i64 - 1) 
        - rx2 * ry2;
    while y >= 0 {
        draw_symmetric(x, y);
        y -= 1;
        py -= two_rx2;
        if p > 0 {
            p += rx2 - py;
        } else {
            x += 1;
            px += two_ry2;
            p += rx2 - py + px;
        }
    }
}

/// Draws a triangle outline
pub fn triangle(x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32, color: Color) {
    line(x1, y1, x2, y2, color);
    line(x2, y2, x3, y3, color);
    line(x3, y3, x1, y1, color);
}

/// Draws a filled triangle using scanline algorithm
pub fn fill_triangle(x1: i32, y1: i32, x2: i32, y2: i32, x3: i32, y3: i32, color: Color) {
    // Sort vertices by y coordinate
    let (mut v1, mut v2, mut v3) = ((x1, y1), (x2, y2), (x3, y3));
    if v1.1 > v2.1 { core::mem::swap(&mut v1, &mut v2); }
    if v1.1 > v3.1 { core::mem::swap(&mut v1, &mut v3); }
    if v2.1 > v3.1 { core::mem::swap(&mut v2, &mut v3); }
    
    let (x1, y1) = v1;
    let (x2, y2) = v2;
    let (x3, y3) = v3;
    
    if y1 == y3 {
        // Degenerate triangle
        let min_x = x1.min(x2).min(x3);
        let max_x = x1.max(x2).max(x3);
        if y1 >= 0 && min_x >= 0 {
            hline(min_x as u32, max_x as u32, y1 as u32, color);
        }
        return;
    }
    
    let draw_scanline = |y: i32, x_start: i32, x_end: i32| {
        if y >= 0 {
            let (start, end) = if x_start < x_end { (x_start, x_end) } else { (x_end, x_start) };
            if start >= 0 {
                hline(start as u32, end as u32, y as u32, color);
            } else if end >= 0 {
                hline(0, end as u32, y as u32, color);
            }
        }
    };
    
    // Calculate slopes
    let inv_slope1 = (x3 - x1) as f32 / (y3 - y1) as f32;
    let inv_slope2 = (x2 - x1) as f32 / (y2 - y1).max(1) as f32;
    let inv_slope3 = (x3 - x2) as f32 / (y3 - y2).max(1) as f32;
    
    let mut cur_x1 = x1 as f32;
    let mut cur_x2 = x1 as f32;
    
    // Bottom flat triangle
    for y in y1..=y2 {
        draw_scanline(y, cur_x1 as i32, cur_x2 as i32);
        cur_x1 += inv_slope1;
        cur_x2 += inv_slope2;
    }
    
    // Top flat triangle
    cur_x2 = x2 as f32;
    for y in y2..=y3 {
        draw_scanline(y, cur_x1 as i32, cur_x2 as i32);
        cur_x1 += inv_slope1;
        cur_x2 += inv_slope3;
    }
}

/// Draws a polygon outline
pub fn polygon(points: &[(i32, i32)], color: Color) {
    if points.len() < 2 {
        return;
    }
    
    for i in 0..points.len() {
        let j = (i + 1) % points.len();
        line(points[i].0, points[i].1, points[j].0, points[j].1, color);
    }
}

/// Draws a gradient rectangle (vertical)
pub fn gradient_rect_v(x: u32, y: u32, width: u32, height: u32, start: Color, end: Color) {
    if height == 0 {
        return;
    }
    
    if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
        for row in 0..height {
            let t = row as f32 / (height - 1).max(1) as f32;
            let color = start.lerp(end, t);
            for col in 0..width {
                fb.set_pixel(x + col, y + row, color);
            }
        }
    }
}

/// Draws a gradient rectangle (horizontal)
pub fn gradient_rect_h(x: u32, y: u32, width: u32, height: u32, start: Color, end: Color) {
    if width == 0 {
        return;
    }
    
    if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
        for col in 0..width {
            let t = col as f32 / (width - 1).max(1) as f32;
            let color = start.lerp(end, t);
            for row in 0..height {
                fb.set_pixel(x + col, y + row, color);
            }
        }
    }
}

/// Draws a bitmap image (1bpp)
pub fn draw_bitmap(x: u32, y: u32, bitmap: &[u8], width: u32, height: u32, fg: Color, bg: Option<Color>) {
    let bytes_per_row = (width + 7) / 8;
    
    if let Some(ref mut fb) = *framebuffer::FRAMEBUFFER.lock() {
        for row in 0..height {
            for col in 0..width {
                let byte_idx = (row * bytes_per_row + col / 8) as usize;
                let bit_idx = 7 - (col % 8);
                
                if byte_idx < bitmap.len() {
                    let set = (bitmap[byte_idx] >> bit_idx) & 1 != 0;
                    if set {
                        fb.set_pixel(x + col, y + row, fg);
                    } else if let Some(bg_color) = bg {
                        fb.set_pixel(x + col, y + row, bg_color);
                    }
                }
            }
        }
    }
}
