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
        description: 'Tunnel R.A.T. when Device UI is set to Tunnel R.A.T., otherwise Rayhunter.',
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
        name: 'Tunnel R.A.T. Light',
        description: 'Dark header with the rat, light page.',
        swatch: ['#14202b', '#ffffff', '#3ddc84'],
    },
    {
        id: 'tunnel-rat-dark',
        name: 'Tunnel R.A.T. Dark',
        description: 'Terminal green on near-black, like the Tunnel R.A.T. dashboard.',
        swatch: ['#0f1a14', '#080d0a', '#39ff88'],
    },
];

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
