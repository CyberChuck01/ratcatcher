<script lang="ts">
    import { THEMES, type ThemeChoice } from '$lib/trTheme';
    import Modal from './Modal.svelte';

    let {
        shown = $bindable(),
        choice,
        onchoose,
    }: { shown: boolean; choice: ThemeChoice; onchoose: (choice: ThemeChoice) => void } = $props();
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
    </div>
</Modal>
