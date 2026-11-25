/**
 * Ported from KuroCat (https://github.com/qianyiai/kurocat), v5.1.0.
 * Copyright (c) 2026 KuroCat Contributors, MIT License.
 * See src-tauri/resources/licenses/KUROCAT-MIT.txt.
 */
export type KeyboardLayoutId = '65' | '108';

export interface KeySpec {
  id: string;
  label: string;
  bubble: string;
  row: number;
  rowY?: number; // exact vertical row position in key units (defaults to row)
  rowSpan?: number; // vertical span in key units (defaults to 1; 2 for Numpad + and Numpad Enter)
  uStart: number; // horizontal unit column start
  uWidth: number; // width in standard 1U key units
  accent?: 'orange' | 'mod' | 'normal';
  vkCodes: number[];
}

export interface KeyboardLayoutDef {
  id: KeyboardLayoutId;
  totalUnits: number;
  totalRows: number;
  keys: KeySpec[];
}

export const VK_NUMPAD_ENTER = 0x10d;

export const TOTAL_UNITS = 15;
export const TOTAL_ROWS = 5;

export const KEYBOARD_KEYS_65: KeySpec[] = [
  // Row 0: Orange Esc (rotates 180° to bottom-right corner facing cat!) + 1..0 + - + = + Backspace (15U total)
  { id: 'esc', label: 'ESC', bubble: 'Esc', row: 0, uStart: 0, uWidth: 1, accent: 'orange', vkCodes: [0x1b, 0xc0] },
  { id: '1', label: '1', bubble: '1', row: 0, uStart: 1, uWidth: 1, vkCodes: [0x31, 0x61, 0x70] },
  { id: '2', label: '2', bubble: '2', row: 0, uStart: 2, uWidth: 1, vkCodes: [0x32, 0x62, 0x71] },
  { id: '3', label: '3', bubble: '3', row: 0, uStart: 3, uWidth: 1, vkCodes: [0x33, 0x63, 0x72] },
  { id: '4', label: '4', bubble: '4', row: 0, uStart: 4, uWidth: 1, vkCodes: [0x34, 0x64, 0x73] },
  { id: '5', label: '5', bubble: '5', row: 0, uStart: 5, uWidth: 1, vkCodes: [0x35, 0x65, 0x74] },
  { id: '6', label: '6', bubble: '6', row: 0, uStart: 6, uWidth: 1, vkCodes: [0x36, 0x66, 0x75] },
  { id: '7', label: '7', bubble: '7', row: 0, uStart: 7, uWidth: 1, vkCodes: [0x37, 0x67, 0x76] },
  { id: '8', label: '8', bubble: '8', row: 0, uStart: 8, uWidth: 1, vkCodes: [0x38, 0x68, 0x77] },
  { id: '9', label: '9', bubble: '9', row: 0, uStart: 9, uWidth: 1, vkCodes: [0x39, 0x69, 0x78] },
  { id: '0', label: '0', bubble: '0', row: 0, uStart: 10, uWidth: 1, vkCodes: [0x30, 0x60, 0x79] },
  { id: 'minus', label: '-', bubble: '-', row: 0, uStart: 11, uWidth: 1, vkCodes: [0xbd, 0x6d, 0x7a] },
  { id: 'equal', label: '=', bubble: '=', row: 0, uStart: 12, uWidth: 1, vkCodes: [0xbb, 0x6b, 0x7b] },
  { id: 'bs', label: '⌫', bubble: '⌫', row: 0, uStart: 13, uWidth: 2, accent: 'mod', vkCodes: [0x08, 0x2e] },

  // Row 1: Tab + Q..P + [ ] \ (15U total)
  { id: 'tab', label: '↹', bubble: 'Tab', row: 1, uStart: 0, uWidth: 1.5, accent: 'mod', vkCodes: [0x09] },
  { id: 'q', label: 'Q', bubble: 'Q', row: 1, uStart: 1.5, uWidth: 1, vkCodes: [0x51] },
  { id: 'w', label: 'W', bubble: 'W', row: 1, uStart: 2.5, uWidth: 1, vkCodes: [0x57] },
  { id: 'e', label: 'E', bubble: 'E', row: 1, uStart: 3.5, uWidth: 1, vkCodes: [0x45] },
  { id: 'r', label: 'R', bubble: 'R', row: 1, uStart: 4.5, uWidth: 1, vkCodes: [0x52] },
  { id: 't', label: 'T', bubble: 'T', row: 1, uStart: 5.5, uWidth: 1, vkCodes: [0x54] },
  { id: 'y', label: 'Y', bubble: 'Y', row: 1, uStart: 6.5, uWidth: 1, vkCodes: [0x59] },
  { id: 'u', label: 'U', bubble: 'U', row: 1, uStart: 7.5, uWidth: 1, vkCodes: [0x55] },
  { id: 'i', label: 'I', bubble: 'I', row: 1, uStart: 8.5, uWidth: 1, vkCodes: [0x49] },
  { id: 'o', label: 'O', bubble: 'O', row: 1, uStart: 9.5, uWidth: 1, vkCodes: [0x4f] },
  { id: 'p', label: 'P', bubble: 'P', row: 1, uStart: 10.5, uWidth: 1, vkCodes: [0x50] },
  { id: 'lb', label: '[', bubble: '[', row: 1, uStart: 11.5, uWidth: 1, vkCodes: [0xdb] },
  { id: 'rb', label: ']', bubble: ']', row: 1, uStart: 12.5, uWidth: 1, vkCodes: [0xdd] },
  { id: 'bslash', label: '\\', bubble: '\\', row: 1, uStart: 13.5, uWidth: 1.5, accent: 'mod', vkCodes: [0xdc, 0x2d, 0x24] },

  // Row 2: Caps + A..L + ; ' + Enter (15U total)
  { id: 'caps', label: '⇪', bubble: 'Caps', row: 2, uStart: 0, uWidth: 1.75, accent: 'mod', vkCodes: [0x14] },
  { id: 'a', label: 'A', bubble: 'A', row: 2, uStart: 1.75, uWidth: 1, vkCodes: [0x41] },
  { id: 's', label: 'S', bubble: 'S', row: 2, uStart: 2.75, uWidth: 1, vkCodes: [0x53] },
  { id: 'd', label: 'D', bubble: 'D', row: 2, uStart: 3.75, uWidth: 1, vkCodes: [0x44] },
  { id: 'f', label: 'F', bubble: 'F', row: 2, uStart: 4.75, uWidth: 1, vkCodes: [0x46] },
  { id: 'g', label: 'G', bubble: 'G', row: 2, uStart: 5.75, uWidth: 1, vkCodes: [0x47] },
  { id: 'h', label: 'H', bubble: 'H', row: 2, uStart: 6.75, uWidth: 1, vkCodes: [0x48] },
  { id: 'j', label: 'J', bubble: 'J', row: 2, uStart: 7.75, uWidth: 1, vkCodes: [0x4a] },
  { id: 'k', label: 'K', bubble: 'K', row: 2, uStart: 8.75, uWidth: 1, vkCodes: [0x4b] },
  { id: 'l', label: 'L', bubble: 'L', row: 2, uStart: 9.75, uWidth: 1, vkCodes: [0x4c] },
  { id: 'semi', label: ';', bubble: ';', row: 2, uStart: 10.75, uWidth: 1, vkCodes: [0xba] },
  { id: 'quote', label: "'", bubble: "'", row: 2, uStart: 11.75, uWidth: 1, vkCodes: [0xde] },
  { id: 'enter', label: '↵', bubble: '↵', row: 2, uStart: 12.75, uWidth: 2.25, accent: 'mod', vkCodes: [0x0d, VK_NUMPAD_ENTER] },

  // Row 3: LShift + Z..M + , . / + Orange RShift (rotates 180° to iconic upper-left orange bar facing cat!) (15U total)
  { id: 'lshift', label: '⇧', bubble: '⇧', row: 3, uStart: 0, uWidth: 2.25, accent: 'mod', vkCodes: [0xa0, 0x10] },
  { id: 'z', label: 'Z', bubble: 'Z', row: 3, uStart: 2.25, uWidth: 1, vkCodes: [0x5a] },
  { id: 'x', label: 'X', bubble: 'X', row: 3, uStart: 3.25, uWidth: 1, vkCodes: [0x58] },
  { id: 'c', label: 'C', bubble: 'C', row: 3, uStart: 4.25, uWidth: 1, vkCodes: [0x43] },
  { id: 'v', label: 'V', bubble: 'V', row: 3, uStart: 5.25, uWidth: 1, vkCodes: [0x56] },
  { id: 'b', label: 'B', bubble: 'B', row: 3, uStart: 6.25, uWidth: 1, vkCodes: [0x42] },
  { id: 'n', label: 'N', bubble: 'N', row: 3, uStart: 7.25, uWidth: 1, vkCodes: [0x4e] },
  { id: 'm', label: 'M', bubble: 'M', row: 3, uStart: 8.25, uWidth: 1, vkCodes: [0x4d] },
  { id: 'comma', label: ',', bubble: ',', row: 3, uStart: 9.25, uWidth: 1, vkCodes: [0xbc] },
  { id: 'dot', label: '.', bubble: '.', row: 3, uStart: 10.25, uWidth: 1, vkCodes: [0xbe, 0x6e] },
  { id: 'slash', label: '/', bubble: '/', row: 3, uStart: 11.25, uWidth: 1, vkCodes: [0xbf, 0x6f] },
  { id: 'rshift', label: '⇧', bubble: '⇧', row: 3, uStart: 12.25, uWidth: 2.75, accent: 'orange', vkCodes: [0xa1] },

  // Row 4: Ctrl Win Alt Space Alt Left Down Up Right (rotates 180° to top row closest to cat!) (15U total)
  { id: 'lctrl', label: 'Ctrl', bubble: 'Ctrl', row: 4, uStart: 0, uWidth: 1.25, accent: 'mod', vkCodes: [0xa2, 0x11] },
  { id: 'lwin', label: 'Win', bubble: 'Win', row: 4, uStart: 1.25, uWidth: 1.25, accent: 'mod', vkCodes: [0x5b, 0x5c] },
  { id: 'lalt', label: 'Alt', bubble: 'Alt', row: 4, uStart: 2.5, uWidth: 1.25, accent: 'mod', vkCodes: [0xa4, 0x12] },
  { id: 'space', label: '␣', bubble: '␣', row: 4, uStart: 3.75, uWidth: 6.25, vkCodes: [0x20] },
  { id: 'ralt', label: 'Alt', bubble: 'Alt', row: 4, uStart: 10, uWidth: 1, accent: 'mod', vkCodes: [0xa5, 0xa3] },
  { id: 'left', label: '←', bubble: '←', row: 4, uStart: 11, uWidth: 1, accent: 'mod', vkCodes: [0x25, 0x23] },
  { id: 'down', label: '↓', bubble: '↓', row: 4, uStart: 12, uWidth: 1, accent: 'mod', vkCodes: [0x28, 0x22] },
  { id: 'up', label: '↑', bubble: '↑', row: 4, uStart: 13, uWidth: 1, accent: 'mod', vkCodes: [0x26, 0x21] },
  { id: 'right', label: '→', bubble: '→', row: 4, uStart: 14, uWidth: 1, accent: 'mod', vkCodes: [0x27] },
];

// Authentic full-size 108-key ANSI mechanical keyboard (22.7U x 6.25 rows; 20 + 21 + 21 + 16 + 17 + 13 = 108 keys!)
const NAV_X = 15.35;
const NUM_X = 18.7;

export const KEYBOARD_KEYS_108: KeySpec[] = [
  // Row 0 (rowY=0): Esc + F1..F12 + Prt/Scr/Pau + 4 signature 108-key media/utility keys above Numpad (20 keys)
  { id: 'esc', label: 'Esc', bubble: 'Esc', row: 0, rowY: 0, uStart: 0, uWidth: 1, accent: 'orange', vkCodes: [0x1b] },
  { id: 'f1', label: 'F1', bubble: 'F1', row: 0, rowY: 0, uStart: 2, uWidth: 1, vkCodes: [0x70] },
  { id: 'f2', label: 'F2', bubble: 'F2', row: 0, rowY: 0, uStart: 3, uWidth: 1, vkCodes: [0x71] },
  { id: 'f3', label: 'F3', bubble: 'F3', row: 0, rowY: 0, uStart: 4, uWidth: 1, vkCodes: [0x72] },
  { id: 'f4', label: 'F4', bubble: 'F4', row: 0, rowY: 0, uStart: 5, uWidth: 1, vkCodes: [0x73] },
  { id: 'f5', label: 'F5', bubble: 'F5', row: 0, rowY: 0, uStart: 6.5, uWidth: 1, accent: 'mod', vkCodes: [0x74] },
  { id: 'f6', label: 'F6', bubble: 'F6', row: 0, rowY: 0, uStart: 7.5, uWidth: 1, accent: 'mod', vkCodes: [0x75] },
  { id: 'f7', label: 'F7', bubble: 'F7', row: 0, rowY: 0, uStart: 8.5, uWidth: 1, accent: 'mod', vkCodes: [0x76] },
  { id: 'f8', label: 'F8', bubble: 'F8', row: 0, rowY: 0, uStart: 9.5, uWidth: 1, accent: 'mod', vkCodes: [0x77] },
  { id: 'f9', label: 'F9', bubble: 'F9', row: 0, rowY: 0, uStart: 11, uWidth: 1, vkCodes: [0x78] },
  { id: 'f10', label: 'F10', bubble: 'F10', row: 0, rowY: 0, uStart: 12, uWidth: 1, vkCodes: [0x79] },
  { id: 'f11', label: 'F11', bubble: 'F11', row: 0, rowY: 0, uStart: 13, uWidth: 1, vkCodes: [0x7a] },
  { id: 'f12', label: 'F12', bubble: 'F12', row: 0, rowY: 0, uStart: 14, uWidth: 1, vkCodes: [0x7b] },
  { id: 'prt', label: 'Prt', bubble: 'PrtSc', row: 0, rowY: 0, uStart: NAV_X, uWidth: 1, accent: 'mod', vkCodes: [0x2c] },
  { id: 'scr', label: 'Scr', bubble: 'ScrLk', row: 0, rowY: 0, uStart: NAV_X + 1, uWidth: 1, accent: 'mod', vkCodes: [0x91] },
  { id: 'pau', label: 'Pau', bubble: 'Pause', row: 0, rowY: 0, uStart: NAV_X + 2, uWidth: 1, accent: 'mod', vkCodes: [0x13] },
  { id: 'm_mute', label: 'M1', bubble: 'Mute', row: 0, rowY: 0, uStart: NUM_X, uWidth: 1, accent: 'mod', vkCodes: [0xad] },
  { id: 'm_vdown', label: 'M2', bubble: 'Vol-', row: 0, rowY: 0, uStart: NUM_X + 1, uWidth: 1, accent: 'mod', vkCodes: [0xae] },
  { id: 'm_vup', label: 'M3', bubble: 'Vol+', row: 0, rowY: 0, uStart: NUM_X + 2, uWidth: 1, accent: 'mod', vkCodes: [0xaf] },
  { id: 'm_calc', label: 'M4', bubble: 'Calc', row: 0, rowY: 0, uStart: NUM_X + 3, uWidth: 1, accent: 'mod', vkCodes: [0xb7, 0xb3] },

  // Row 1 (rowY=1.25): ` 1..0 - = Backspace | Ins Hom PgU | Num / * - (21 keys)
  { id: 'grave', label: '`', bubble: '`', row: 1, rowY: 1.25, uStart: 0, uWidth: 1, vkCodes: [0xc0] },
  { id: '1', label: '1', bubble: '1', row: 1, rowY: 1.25, uStart: 1, uWidth: 1, vkCodes: [0x31] },
  { id: '2', label: '2', bubble: '2', row: 1, rowY: 1.25, uStart: 2, uWidth: 1, vkCodes: [0x32] },
  { id: '3', label: '3', bubble: '3', row: 1, rowY: 1.25, uStart: 3, uWidth: 1, vkCodes: [0x33] },
  { id: '4', label: '4', bubble: '4', row: 1, rowY: 1.25, uStart: 4, uWidth: 1, vkCodes: [0x34] },
  { id: '5', label: '5', bubble: '5', row: 1, rowY: 1.25, uStart: 5, uWidth: 1, vkCodes: [0x35] },
  { id: '6', label: '6', bubble: '6', row: 1, rowY: 1.25, uStart: 6, uWidth: 1, vkCodes: [0x36] },
  { id: '7', label: '7', bubble: '7', row: 1, rowY: 1.25, uStart: 7, uWidth: 1, vkCodes: [0x37] },
  { id: '8', label: '8', bubble: '8', row: 1, rowY: 1.25, uStart: 8, uWidth: 1, vkCodes: [0x38] },
  { id: '9', label: '9', bubble: '9', row: 1, rowY: 1.25, uStart: 9, uWidth: 1, vkCodes: [0x39] },
  { id: '0', label: '0', bubble: '0', row: 1, rowY: 1.25, uStart: 10, uWidth: 1, vkCodes: [0x30] },
  { id: 'minus', label: '-', bubble: '-', row: 1, rowY: 1.25, uStart: 11, uWidth: 1, vkCodes: [0xbd] },
  { id: 'equal', label: '=', bubble: '=', row: 1, rowY: 1.25, uStart: 12, uWidth: 1, vkCodes: [0xbb] },
  { id: 'bs', label: '⌫', bubble: '⌫', row: 1, rowY: 1.25, uStart: 13, uWidth: 2, accent: 'mod', vkCodes: [0x08] },
  { id: 'ins', label: 'Ins', bubble: 'Ins', row: 1, rowY: 1.25, uStart: NAV_X, uWidth: 1, accent: 'mod', vkCodes: [0x2d] },
  { id: 'home', label: 'Hm', bubble: 'Home', row: 1, rowY: 1.25, uStart: NAV_X + 1, uWidth: 1, accent: 'mod', vkCodes: [0x24] },
  { id: 'pgup', label: 'PU', bubble: 'PgUp', row: 1, rowY: 1.25, uStart: NAV_X + 2, uWidth: 1, accent: 'mod', vkCodes: [0x21] },
  { id: 'numlk', label: 'Num', bubble: 'Num', row: 1, rowY: 1.25, uStart: NUM_X, uWidth: 1, accent: 'mod', vkCodes: [0x90] },
  { id: 'np_div', label: '/', bubble: 'Num/', row: 1, rowY: 1.25, uStart: NUM_X + 1, uWidth: 1, accent: 'mod', vkCodes: [0x6f] },
  { id: 'np_mul', label: '*', bubble: 'Num*', row: 1, rowY: 1.25, uStart: NUM_X + 2, uWidth: 1, accent: 'mod', vkCodes: [0x6a] },
  { id: 'np_sub', label: '-', bubble: 'Num-', row: 1, rowY: 1.25, uStart: NUM_X + 3, uWidth: 1, accent: 'mod', vkCodes: [0x6d] },

  // Row 2 (rowY=2.25): Tab Q..P [ ] \ | Del End PgD | 7 8 9 +(2U tall) (21 keys)
  { id: 'tab', label: '↹', bubble: 'Tab', row: 2, rowY: 2.25, uStart: 0, uWidth: 1.5, accent: 'mod', vkCodes: [0x09] },
  { id: 'q', label: 'Q', bubble: 'Q', row: 2, rowY: 2.25, uStart: 1.5, uWidth: 1, vkCodes: [0x51] },
  { id: 'w', label: 'W', bubble: 'W', row: 2, rowY: 2.25, uStart: 2.5, uWidth: 1, vkCodes: [0x57] },
  { id: 'e', label: 'E', bubble: 'E', row: 2, rowY: 2.25, uStart: 3.5, uWidth: 1, vkCodes: [0x45] },
  { id: 'r', label: 'R', bubble: 'R', row: 2, rowY: 2.25, uStart: 4.5, uWidth: 1, vkCodes: [0x52] },
  { id: 't', label: 'T', bubble: 'T', row: 2, rowY: 2.25, uStart: 5.5, uWidth: 1, vkCodes: [0x54] },
  { id: 'y', label: 'Y', bubble: 'Y', row: 2, rowY: 2.25, uStart: 6.5, uWidth: 1, vkCodes: [0x59] },
  { id: 'u', label: 'U', bubble: 'U', row: 2, rowY: 2.25, uStart: 7.5, uWidth: 1, vkCodes: [0x55] },
  { id: 'i', label: 'I', bubble: 'I', row: 2, rowY: 2.25, uStart: 8.5, uWidth: 1, vkCodes: [0x49] },
  { id: 'o', label: 'O', bubble: 'O', row: 2, rowY: 2.25, uStart: 9.5, uWidth: 1, vkCodes: [0x4f] },
  { id: 'p', label: 'P', bubble: 'P', row: 2, rowY: 2.25, uStart: 10.5, uWidth: 1, vkCodes: [0x50] },
  { id: 'lb', label: '[', bubble: '[', row: 2, rowY: 2.25, uStart: 11.5, uWidth: 1, vkCodes: [0xdb] },
  { id: 'rb', label: ']', bubble: ']', row: 2, rowY: 2.25, uStart: 12.5, uWidth: 1, vkCodes: [0xdd] },
  { id: 'bslash', label: '\\', bubble: '\\', row: 2, rowY: 2.25, uStart: 13.5, uWidth: 1.5, accent: 'mod', vkCodes: [0xdc] },
  { id: 'del', label: 'Del', bubble: 'Del', row: 2, rowY: 2.25, uStart: NAV_X, uWidth: 1, accent: 'mod', vkCodes: [0x2e] },
  { id: 'end', label: 'End', bubble: 'End', row: 2, rowY: 2.25, uStart: NAV_X + 1, uWidth: 1, accent: 'mod', vkCodes: [0x23] },
  { id: 'pgdn', label: 'PD', bubble: 'PgDn', row: 2, rowY: 2.25, uStart: NAV_X + 2, uWidth: 1, accent: 'mod', vkCodes: [0x22] },
  { id: 'np_7', label: '7', bubble: 'Num7', row: 2, rowY: 2.25, uStart: NUM_X, uWidth: 1, vkCodes: [0x67] },
  { id: 'np_8', label: '8', bubble: 'Num8', row: 2, rowY: 2.25, uStart: NUM_X + 1, uWidth: 1, vkCodes: [0x68] },
  { id: 'np_9', label: '9', bubble: 'Num9', row: 2, rowY: 2.25, uStart: NUM_X + 2, uWidth: 1, vkCodes: [0x69] },
  { id: 'np_add', label: '+', bubble: 'Num+', row: 2, rowY: 2.25, rowSpan: 2, uStart: NUM_X + 3, uWidth: 1, accent: 'mod', vkCodes: [0x6b] },

  // Row 3 (rowY=3.25): Caps A..L ; ' Enter | (nav gap) | 4 5 6 (16 keys)
  { id: 'caps', label: '⇪', bubble: 'Caps', row: 3, rowY: 3.25, uStart: 0, uWidth: 1.75, accent: 'mod', vkCodes: [0x14] },
  { id: 'a', label: 'A', bubble: 'A', row: 3, rowY: 3.25, uStart: 1.75, uWidth: 1, vkCodes: [0x41] },
  { id: 's', label: 'S', bubble: 'S', row: 3, rowY: 3.25, uStart: 2.75, uWidth: 1, vkCodes: [0x53] },
  { id: 'd', label: 'D', bubble: 'D', row: 3, rowY: 3.25, uStart: 3.75, uWidth: 1, vkCodes: [0x44] },
  { id: 'f', label: 'F', bubble: 'F', row: 3, rowY: 3.25, uStart: 4.75, uWidth: 1, vkCodes: [0x46] },
  { id: 'g', label: 'G', bubble: 'G', row: 3, rowY: 3.25, uStart: 5.75, uWidth: 1, vkCodes: [0x47] },
  { id: 'h', label: 'H', bubble: 'H', row: 3, rowY: 3.25, uStart: 6.75, uWidth: 1, vkCodes: [0x48] },
  { id: 'j', label: 'J', bubble: 'J', row: 3, rowY: 3.25, uStart: 7.75, uWidth: 1, vkCodes: [0x4a] },
  { id: 'k', label: 'K', bubble: 'K', row: 3, rowY: 3.25, uStart: 8.75, uWidth: 1, vkCodes: [0x4b] },
  { id: 'l', label: 'L', bubble: 'L', row: 3, rowY: 3.25, uStart: 9.75, uWidth: 1, vkCodes: [0x4c] },
  { id: 'semi', label: ';', bubble: ';', row: 3, rowY: 3.25, uStart: 10.75, uWidth: 1, vkCodes: [0xba] },
  { id: 'quote', label: "'", bubble: "'", row: 3, rowY: 3.25, uStart: 11.75, uWidth: 1, vkCodes: [0xde] },
  { id: 'enter', label: '↵', bubble: '↵', row: 3, rowY: 3.25, uStart: 12.75, uWidth: 2.25, accent: 'mod', vkCodes: [0x0d] },
  { id: 'np_4', label: '4', bubble: 'Num4', row: 3, rowY: 3.25, uStart: NUM_X, uWidth: 1, vkCodes: [0x64] },
  { id: 'np_5', label: '5', bubble: 'Num5', row: 3, rowY: 3.25, uStart: NUM_X + 1, uWidth: 1, vkCodes: [0x65] },
  { id: 'np_6', label: '6', bubble: 'Num6', row: 3, rowY: 3.25, uStart: NUM_X + 2, uWidth: 1, vkCodes: [0x66] },

  // Row 4 (rowY=4.25): LShift Z..M , . / RShift(orange) | Up | 1 2 3 NumEnter(2U tall, orange!) (17 keys)
  { id: 'lshift', label: '⇧', bubble: '⇧', row: 4, rowY: 4.25, uStart: 0, uWidth: 2.25, accent: 'mod', vkCodes: [0xa0, 0x10] },
  { id: 'z', label: 'Z', bubble: 'Z', row: 4, rowY: 4.25, uStart: 2.25, uWidth: 1, vkCodes: [0x5a] },
  { id: 'x', label: 'X', bubble: 'X', row: 4, rowY: 4.25, uStart: 3.25, uWidth: 1, vkCodes: [0x58] },
  { id: 'c', label: 'C', bubble: 'C', row: 4, rowY: 4.25, uStart: 4.25, uWidth: 1, vkCodes: [0x43] },
  { id: 'v', label: 'V', bubble: 'V', row: 4, rowY: 4.25, uStart: 5.25, uWidth: 1, vkCodes: [0x56] },
  { id: 'b', label: 'B', bubble: 'B', row: 4, rowY: 4.25, uStart: 6.25, uWidth: 1, vkCodes: [0x42] },
  { id: 'n', label: 'N', bubble: 'N', row: 4, rowY: 4.25, uStart: 7.25, uWidth: 1, vkCodes: [0x4e] },
  { id: 'm', label: 'M', bubble: 'M', row: 4, rowY: 4.25, uStart: 8.25, uWidth: 1, vkCodes: [0x4d] },
  { id: 'comma', label: ',', bubble: ',', row: 4, rowY: 4.25, uStart: 9.25, uWidth: 1, vkCodes: [0xbc] },
  { id: 'dot', label: '.', bubble: '.', row: 4, rowY: 4.25, uStart: 10.25, uWidth: 1, vkCodes: [0xbe] },
  { id: 'slash', label: '/', bubble: '/', row: 4, rowY: 4.25, uStart: 11.25, uWidth: 1, vkCodes: [0xbf] },
  { id: 'rshift', label: '⇧', bubble: '⇧', row: 4, rowY: 4.25, uStart: 12.25, uWidth: 2.75, accent: 'orange', vkCodes: [0xa1] },
  { id: 'up', label: '↑', bubble: '↑', row: 4, rowY: 4.25, uStart: NAV_X + 1, uWidth: 1, accent: 'mod', vkCodes: [0x26] },
  { id: 'np_1', label: '1', bubble: 'Num1', row: 4, rowY: 4.25, uStart: NUM_X, uWidth: 1, vkCodes: [0x61] },
  { id: 'np_2', label: '2', bubble: 'Num2', row: 4, rowY: 4.25, uStart: NUM_X + 1, uWidth: 1, vkCodes: [0x62] },
  { id: 'np_3', label: '3', bubble: 'Num3', row: 4, rowY: 4.25, uStart: NUM_X + 2, uWidth: 1, vkCodes: [0x63] },
  { id: 'np_ent', label: '↵', bubble: 'Num↵', row: 4, rowY: 4.25, rowSpan: 2, uStart: NUM_X + 3, uWidth: 1, accent: 'orange', vkCodes: [VK_NUMPAD_ENTER] },

  // Row 5 (rowY=5.25): Ctrl Win Alt Space Alt Win Menu Ctrl | Left Down Right | 0(2U) . (13 keys)
  { id: 'lctrl', label: 'Ctrl', bubble: 'Ctrl', row: 5, rowY: 5.25, uStart: 0, uWidth: 1.25, accent: 'mod', vkCodes: [0xa2, 0x11] },
  { id: 'lwin', label: 'Win', bubble: 'Win', row: 5, rowY: 5.25, uStart: 1.25, uWidth: 1.25, accent: 'mod', vkCodes: [0x5b] },
  { id: 'lalt', label: 'Alt', bubble: 'Alt', row: 5, rowY: 5.25, uStart: 2.5, uWidth: 1.25, accent: 'mod', vkCodes: [0xa4, 0x12] },
  { id: 'space', label: '␣', bubble: '␣', row: 5, rowY: 5.25, uStart: 3.75, uWidth: 6.25, vkCodes: [0x20] },
  { id: 'ralt', label: 'Alt', bubble: 'Alt', row: 5, rowY: 5.25, uStart: 10, uWidth: 1.25, accent: 'mod', vkCodes: [0xa5] },
  { id: 'rwin', label: 'Win', bubble: 'Win', row: 5, rowY: 5.25, uStart: 11.25, uWidth: 1.25, accent: 'mod', vkCodes: [0x5c] },
  { id: 'menu', label: '▤', bubble: 'Menu', row: 5, rowY: 5.25, uStart: 12.5, uWidth: 1.25, accent: 'mod', vkCodes: [0x5d] },
  { id: 'rctrl', label: 'Ctrl', bubble: 'Ctrl', row: 5, rowY: 5.25, uStart: 13.75, uWidth: 1.25, accent: 'mod', vkCodes: [0xa3] },
  { id: 'left', label: '←', bubble: '←', row: 5, rowY: 5.25, uStart: NAV_X, uWidth: 1, accent: 'mod', vkCodes: [0x25] },
  { id: 'down', label: '↓', bubble: '↓', row: 5, rowY: 5.25, uStart: NAV_X + 1, uWidth: 1, accent: 'mod', vkCodes: [0x28] },
  { id: 'right', label: '→', bubble: '→', row: 5, rowY: 5.25, uStart: NAV_X + 2, uWidth: 1, accent: 'mod', vkCodes: [0x27] },
  { id: 'np_0', label: '0', bubble: 'Num0', row: 5, rowY: 5.25, uStart: NUM_X, uWidth: 2, vkCodes: [0x60] },
  { id: 'np_dot', label: '.', bubble: 'Num.', row: 5, rowY: 5.25, uStart: NUM_X + 2, uWidth: 1, vkCodes: [0x6e] },
];

export const KEYBOARD_KEYS = KEYBOARD_KEYS_65;

export const KEYBOARD_LAYOUTS: Record<KeyboardLayoutId, KeyboardLayoutDef> = {
  '65': {
    id: '65',
    totalUnits: 15,
    totalRows: 5,
    keys: KEYBOARD_KEYS_65,
  },
  '108': {
    id: '108',
    totalUnits: 22.7,
    totalRows: 6.25,
    keys: KEYBOARD_KEYS_108,
  },
};

const vkLookups: Record<KeyboardLayoutId, Map<number, KeySpec>> = {
  '65': new Map<number, KeySpec>(),
  '108': new Map<number, KeySpec>(),
};

for (const layoutId of ['65', '108'] as const) {
  for (const key of KEYBOARD_LAYOUTS[layoutId].keys) {
    for (const vk of key.vkCodes) {
      vkLookups[layoutId].set(vk, key);
    }
  }
}

export function getKeyboardLayout(layoutId: KeyboardLayoutId): KeyboardLayoutDef {
  return KEYBOARD_LAYOUTS[layoutId] ?? KEYBOARD_LAYOUTS['65'];
}

export function resolveKeyByVk(vk: number, layoutId: KeyboardLayoutId = '65'): KeySpec | null {
  const direct = vkLookups[layoutId].get(vk);
  if (direct) return direct;
  if (layoutId === '65' && vk >= 0x70 && vk <= 0x87) {
    const idx = Math.min(12, Math.max(1, vk - 0x6f));
    return KEYBOARD_KEYS_65[idx] ?? KEYBOARD_KEYS_65[0];
  }
  return null;
}

export function formatBubbleForVk(vk: number, fallbackSpec: KeySpec | null): string {
  if (fallbackSpec?.bubble) return fallbackSpec.bubble;
  if (vk >= 0x70 && vk <= 0x7b) {
    return `F${vk - 0x6f}`;
  }
  if (vk === VK_NUMPAD_ENTER) return 'Num↵';
  if (vk === 0xc0) return '`';
  if (vk === 0x2e) return 'Del';
  if (vk === 0x24) return 'Home';
  if (vk === 0x23) return 'End';
  if (vk === 0x21) return 'PgUp';
  if (vk === 0x22) return 'PgDn';
  return '';
}
