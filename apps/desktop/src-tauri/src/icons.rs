//! 托盘图标：4 种状态用不同的颜色和形状，不依赖外部图片文件。

use crate::state::TrayState;

pub const SIZE: u32 = 32;

type Rgba = [u8; 4];

fn paint(f: impl Fn(f32, f32) -> Option<Rgba>) -> Vec<u8> {
    let mut buf = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            // 以像素中心为准，坐标相对图标中心。
            let (dx, dy) = (x as f32 + 0.5 - SIZE as f32 / 2.0, y as f32 + 0.5 - SIZE as f32 / 2.0);
            buf.extend_from_slice(&f(dx, dy).unwrap_or([0, 0, 0, 0]));
        }
    }
    buf
}

/// 返回 `SIZE × SIZE` 的 RGBA 像素。
pub fn rgba(state: TrayState) -> Vec<u8> {
    const WHITE: Rgba = [255, 255, 255, 255];
    match state {
        // 已同步：实心绿圆
        TrayState::Synced => paint(|dx, dy| (dx * dx + dy * dy <= 13.0 * 13.0).then_some([34, 197, 94, 255])),
        // 同步中：蓝圆，中间一个白点
        TrayState::Syncing => paint(|dx, dy| {
            let d2 = dx * dx + dy * dy;
            if d2 <= 5.0 * 5.0 {
                Some(WHITE)
            } else if d2 <= 13.0 * 13.0 {
                Some([59, 130, 246, 255])
            } else {
                None
            }
        }),
        // 离线：灰色空心圆环
        TrayState::Offline => paint(|dx, dy| {
            let d2 = dx * dx + dy * dy;
            (d2 <= 13.0 * 13.0 && d2 >= 8.0 * 8.0).then_some([156, 163, 175, 255])
        }),
        // 出错：红色方块，中间一个白色叉
        TrayState::Error => paint(|dx, dy| {
            if dx.abs() > 12.0 || dy.abs() > 12.0 {
                None
            } else if (dx.abs() - dy.abs()).abs() <= 1.6 && dx.abs() <= 7.0 {
                Some(WHITE)
            } else {
                Some([239, 68, 68, 255])
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_icons_are_pairwise_different() {
        let all = [TrayState::Synced, TrayState::Syncing, TrayState::Offline, TrayState::Error].map(rgba);
        for i in 0..all.len() {
            assert_eq!(all[i].len(), (SIZE * SIZE * 4) as usize);
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j]);
            }
        }
    }
}
