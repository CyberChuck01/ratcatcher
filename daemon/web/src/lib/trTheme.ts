/** Tunnel R.A.T. themes for Rayhunter's web page.
 *
 * The choice is saved in this browser only (localStorage), so it never changes
 * what other viewers see. "auto" follows the device: the Tunnel R.A.T. theme
 * when Device UI is set to "Tunnel R.A.T.", Rayhunter's own look otherwise.
 */

export type ThemeChoice = 'auto' | 'default' | 'tunnel-rat' | 'tunnel-rat-dark';
export type ResolvedTheme = Exclude<ThemeChoice, 'auto'>;

export interface ThemeInfo {
    id: ThemeChoice;
    name: string;
    description: string;
    /** Swatch colors for the preview: header, page, accent. */
    swatch: [string, string, string];
}

export const THEMES: ThemeInfo[] = [
    {
        id: 'auto',
        name: 'Automatic',
        description: 'Ratcatcher when Device UI is set to Ratcatcher, otherwise Rayhunter.',
        swatch: ['#14202b', '#ffffff', '#3ddc84'],
    },
    {
        id: 'default',
        name: 'Rayhunter',
        description: "Rayhunter's original look.",
        swatch: ['#4e4eb1', '#ffffff', '#94ea18'],
    },
    {
        id: 'tunnel-rat',
        name: 'Ratcatcher',
        description: 'Dark header with the rat, light page. Uses the dashboard colors for its header line.',
        swatch: ['#14202b', '#ffffff', '#ff2222'],
    },
    {
        id: 'tunnel-rat-dark',
        name: 'Tunnel R.A.T. Dashboard',
        description:
            "The Tunnel R.A.T. dashboard's look: monospace on near-black, with its color presets below.",
        swatch: ['#0d0000', '#080000', '#ff2222'],
    },
];

/** The Tunnel R.A.T. dashboard's three-color system, ported from dashboard-v14:
 *  accent = headings, borders, active buttons
 *  window = panel and text color
 *  bg     = page background (a deep version of this color) and tab text
 */
export interface DashColors {
    accent: string;
    windowc: string;
    bg: string;
}

export interface ColorPreset extends DashColors {
    id: string;
    name: string;
}

/** The dashboard's presets, same names and values. */
export const COLOR_PRESETS: ColorPreset[] = [
    { id: 'red', name: 'Red Team', accent: '#ff2222', windowc: '#ff6666', bg: '#ff2222' },
    { id: 'green', name: 'Matrix', accent: '#00ff66', windowc: '#66ffaa', bg: '#00ff66' },
    { id: 'blue', name: 'Blue Ops', accent: '#3399ff', windowc: '#88ccff', bg: '#3399ff' },
    { id: 'amber', name: 'Amber', accent: '#ffaa00', windowc: '#ffcc66', bg: '#ffaa00' },
    { id: 'purple', name: 'Purple', accent: '#cc66ff', windowc: '#e0aaff', bg: '#cc66ff' },
    { id: 'white', name: 'White', accent: '#ffffff', windowc: '#dddddd', bg: '#888888' },
    { id: 'black', name: 'Black', accent: '#222222', windowc: '#444444', bg: '#111111' },
];

export const DEFAULT_COLORS: DashColors = COLOR_PRESETS[0];

const COLORS_KEY = 'tunnel-rat-colors';

function hex_to_rgb(hex: string): [number, number, number] {
    const h = hex.replace('#', '');
    return [
        parseInt(h.slice(0, 2), 16) || 0,
        parseInt(h.slice(2, 4), 16) || 0,
        parseInt(h.slice(4, 6), 16) || 0,
    ];
}

function to_hex(n: number): string {
    return Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, '0');
}

function mix(a: string, b: string, t: number): string {
    const A = hex_to_rgb(a);
    const B = hex_to_rgb(b);
    return (
        '#' +
        to_hex(A[0] + (B[0] - A[0]) * t) +
        to_hex(A[1] + (B[1] - A[1]) * t) +
        to_hex(A[2] + (B[2] - A[2]) * t)
    );
}

function luminance(hex: string): number {
    const [r, g, b] = hex_to_rgb(hex);
    return (0.299 * r + 0.587 * g + 0.114 * b) / 255;
}

/** The dashboard's applyColors(), returning CSS variables instead of setting them. */
export function dash_css_vars(c: DashColors): Record<string, string> {
    const page_bg = mix(c.bg, '#000000', 0.85);
    return {
        '--tr-accent': c.accent,
        '--tr-xaccent': mix(c.accent, '#ffffff', 0.25),
        '--tr-dred': mix(c.accent, '#000000', 0.2),
        '--tr-dim': mix(c.accent, page_bg, 0.55),
        '--tr-ddim': mix(c.accent, page_bg, 0.78),
        '--tr-border': mix(c.accent, page_bg, 0.5),
        '--tr-sec': luminance(c.windowc) < 0.35 ? mix(c.windowc, '#ffffff', 0.6) : c.windowc,
        '--tr-card': mix(c.windowc, '#000000', 0.82),
        '--tr-tab': luminance(c.bg) < 0.35 ? mix(c.bg, '#ffffff', 0.55) : c.bg,
        '--tr-bg': page_bg,
    };
}

function valid_hex(v: unknown): v is string {
    return typeof v === 'string' && /^#[0-9a-fA-F]{6}$/.test(v);
}

export function get_colors(): DashColors {
    try {
        const raw = localStorage.getItem(COLORS_KEY);
        if (raw) {
            const c = JSON.parse(raw);
            if (valid_hex(c.accent) && valid_hex(c.windowc) && valid_hex(c.bg)) {
                return { accent: c.accent, windowc: c.windowc, bg: c.bg };
            }
        }
    } catch {
        // storage blocked or bad data: use the default
    }
    return { ...DEFAULT_COLORS };
}

export function save_colors(c: DashColors) {
    try {
        localStorage.setItem(COLORS_KEY, JSON.stringify(c));
    } catch {
        // storage blocked: the choice just won't persist
    }
}

export function apply_colors(c: DashColors) {
    for (const [name, value] of Object.entries(dash_css_vars(c))) {
        document.documentElement.style.setProperty(name, value);
    }
}

const KEY = 'tunnel-rat-theme';

export function get_theme_choice(): ThemeChoice {
    try {
        const v = localStorage.getItem(KEY);
        if (v === 'default' || v === 'tunnel-rat' || v === 'tunnel-rat-dark') {
            return v;
        }
    } catch {
        // storage blocked: fall through to automatic
    }
    return 'auto';
}

export function save_theme_choice(choice: ThemeChoice) {
    try {
        localStorage.setItem(KEY, choice);
    } catch {
        // storage blocked: the choice just won't persist
    }
}

export function resolve_theme(choice: ThemeChoice, ui_level: number | undefined): ResolvedTheme {
    if (choice !== 'auto') {
        return choice;
    }
    return ui_level === 5 ? 'tunnel-rat' : 'default';
}

export function apply_theme(theme: ResolvedTheme) {
    if (theme === 'default') {
        delete document.documentElement.dataset.trTheme;
    } else {
        document.documentElement.dataset.trTheme = theme;
    }
}
