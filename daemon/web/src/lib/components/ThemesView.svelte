<script lang="ts">
    import { COLOR_PRESETS, THEMES, type DashColors, type ThemeChoice } from '$lib/trTheme';
    import Modal from './Modal.svelte';

    let {
        shown = $bindable(),
        choice,
        onchoose,
        colors,
        oncolors,
    }: {
        shown: boolean;
        choice: ThemeChoice;
        onchoose: (choice: ThemeChoice) => void;
        colors: DashColors;
        oncolors: (colors: DashColors) => void;
    } = $props();
</script>

<Modal bind:shown title="Themes">
    <div class="flex flex-col gap-3 overflow-auto">
        <p class="text-sm text-gray-600">
            Saved in this browser only. Other devices keep their own choice.
        </p>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
            {#each THEMES as theme (theme.id)}
                <button
                    type="button"
                    class="text-left border-2 rounded-md p-3 flex flex-col gap-2 {choice === theme.id
                        ? 'border-green-500'
                        : 'border-gray-300'}"
                    aria-pressed={choice === theme.id}
                    onclick={() => onchoose(theme.id)}
                >
                    <div class="flex flex-row h-10 rounded-sm overflow-hidden border border-gray-300">
                        <div class="flex-1" style="background:{theme.swatch[0]}"></div>
                        <div class="flex-1" style="background:{theme.swatch[1]}"></div>
                        <div class="w-10" style="background:{theme.swatch[2]}"></div>
                    </div>
                    <div class="font-semibold">
                        {theme.name}{choice === theme.id ? ' (selected)' : ''}
                    </div>
                    <div class="text-sm text-gray-600">{theme.description}</div>
                </button>
            {/each}
        </div>

        <div class="text-xl mt-2">Dashboard colors</div>
        <p class="text-sm text-gray-600">
            The same color presets as the Tunnel R.A.T. dashboard. They color the Ratcatcher
            themes: the header line in Ratcatcher, the whole page in Tunnel R.A.T. Dashboard.
        </p>
        <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
            {#each COLOR_PRESETS as p (p.id)}
                <button
                    type="button"
                    class="text-left border-2 rounded-md p-3 flex flex-col gap-2 {colors.accent ===
                        p.accent &&
                    colors.windowc === p.windowc &&
                    colors.bg === p.bg
                        ? 'border-green-500'
                        : 'border-gray-300'}"
                    onclick={() => oncolors({ accent: p.accent, windowc: p.windowc, bg: p.bg })}
                >
                    <div class="flex flex-row h-6 rounded-sm overflow-hidden border border-gray-300">
                        <div class="flex-1" style="background:{p.accent}"></div>
                        <div class="flex-1" style="background:{p.windowc}"></div>
                        <div class="flex-1" style="background:{p.bg}"></div>
                    </div>
                    <span class="font-semibold">{p.name}</span>
                </button>
            {/each}
        </div>
        <div class="text-lg mt-1">Custom</div>
        <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
            <label class="flex flex-row items-center gap-2">
                <input
                    type="color"
                    value={colors.accent}
                    oninput={(e) => oncolors({ ...colors, accent: e.currentTarget.value })}
                />
                <span>Accent<br /><span class="text-xs text-gray-600">headings, borders, active buttons</span></span>
            </label>
            <label class="flex flex-row items-center gap-2">
                <input
                    type="color"
                    value={colors.windowc}
                    oninput={(e) => oncolors({ ...colors, windowc: e.currentTarget.value })}
                />
                <span>Window<br /><span class="text-xs text-gray-600">panels and text</span></span>
            </label>
            <label class="flex flex-row items-center gap-2">
                <input
                    type="color"
                    value={colors.bg}
                    oninput={(e) => oncolors({ ...colors, bg: e.currentTarget.value })}
                />
                <span>Background<br /><span class="text-xs text-gray-600">page color and tabs</span></span>
            </label>
        </div>
    </div>
</Modal>
