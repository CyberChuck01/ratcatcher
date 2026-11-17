<script lang="ts">
    import { ACCENTS, THEMES, type AccentChoice, type ThemeChoice } from '$lib/trTheme';
    import Modal from './Modal.svelte';

    let {
        shown = $bindable(),
        choice,
        onchoose,
        accent,
        onaccent,
    }: {
        shown: boolean;
        choice: ThemeChoice;
        onchoose: (choice: ThemeChoice) => void;
        accent: AccentChoice;
        onaccent: (accent: AccentChoice) => void;
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

        <div class="text-xl mt-2">Accent color</div>
        <p class="text-sm text-gray-600">
            The Tunnel R.A.T. rat's status colors. Used for links, buttons and the header line in
            the Tunnel R.A.T. themes.
        </p>
        <div class="grid grid-cols-2 md:grid-cols-3 gap-3">
            {#each ACCENTS as a (a.id)}
                <button
                    type="button"
                    class="text-left border-2 rounded-md p-3 flex flex-row items-center gap-3 {accent ===
                    a.id
                        ? 'border-green-500'
                        : 'border-gray-300'}"
                    aria-pressed={accent === a.id}
                    onclick={() => onaccent(a.id)}
                >
                    <span
                        class="w-8 h-8 rounded-full border border-gray-300 shrink-0"
                        style="background:{a.color}"
                    ></span>
                    <span class="flex flex-col">
                        <span class="font-semibold">{a.name}{accent === a.id ? ' (selected)' : ''}</span>
                        <span class="text-xs text-gray-600">{a.meaning}</span>
                    </span>
                </button>
            {/each}
        </div>
    </div>
</Modal>
