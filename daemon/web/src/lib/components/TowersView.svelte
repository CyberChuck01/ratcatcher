<script lang="ts">
    import { get_tunnel_rat_cells, type TunnelRatCells, type TunnelRatCell } from '$lib/utils.svelte';
    import Modal from './Modal.svelte';

    let { shown = $bindable() }: { shown: boolean } = $props();
    let data: TunnelRatCells | undefined = $state(undefined);
    let error: string | undefined = $state(undefined);

    $effect(() => {
        const interval = setInterval(async () => {
            try {
                if (data !== undefined && (document.hidden || !shown)) {
                    return;
                }
                data = await get_tunnel_rat_cells();
                error = undefined;
            } catch (e) {
                error = e instanceof Error ? e.message : String(e);
            }
        }, 2000);

        return () => clearInterval(interval);
    });

    // Group cells into physical towers: same carrier, tracking area and eNodeB.
    // Cells whose identity hasn't been decoded yet go under "Unidentified".
    type Tower = { key: string; label: string; cells: TunnelRatCell[] };

    let towers: Tower[] = $derived.by(() => {
        const groups = new Map<string, Tower>();
        for (const cell of data?.cells ?? []) {
            const identified = cell.plmn !== null && cell.enb !== null;
            const key = identified ? `${cell.plmn}/${cell.tac}/${cell.enb}` : 'unidentified';
            const label = identified
                ? `${cell.plmn}  TAC ${cell.tac}  eNB ${cell.enb}`
                : 'Unidentified (no SIB1 heard yet)';
            if (!groups.has(key)) {
                groups.set(key, { key, label, cells: [] });
            }
            groups.get(key)!.cells.push(cell);
        }
        const list = [...groups.values()];
        // Unidentified last.
        return list.sort((a, b) =>
            a.key === 'unidentified' ? 1 : b.key === 'unidentified' ? -1 : a.key.localeCompare(b.key),
        );
    });

    let identified_count = $derived(towers.filter((t) => t.key !== 'unidentified').length);

    function csv_href(): string {
        const rows = [
            'plmn,tac,eci,enb,sector,earfcn,pci',
            ...(data?.cells ?? []).map((c) =>
                [c.plmn ?? '', c.tac ?? '', c.eci ?? '', c.enb ?? '', c.sector ?? '', c.earfcn, c.pci].join(
                    ',',
                ),
            ),
        ];
        return 'data:text/csv;charset=utf-8,' + encodeURIComponent(rows.join('\n') + '\n');
    }
</script>

<Modal bind:shown title="Towers">
    <div class="flex flex-col gap-3 overflow-auto">
        {#if error !== undefined}
            <div class="text-red-700">Couldn't read towers: {error}</div>
        {:else if data === undefined}
            <div>Loading...</div>
        {:else}
            <div class="flex flex-row flex-wrap gap-x-6 gap-y-1">
                <div><span class="font-semibold">{data.cells.length}</span> cells</div>
                <div><span class="font-semibold">{identified_count}</span> identified towers</div>
                <div><span class="font-semibold">{data.stingrays}</span> stingray alerts</div>
                <div>Link: {data.link}</div>
                <div>{data.recording ? 'Recording' : 'Not recording'}</div>
                <a
                    class="underline ml-auto"
                    href={csv_href()}
                    download="tunnel-rat-orbic-cells.csv">Download CSV</a
                >
            </div>
            {#each towers as tower (tower.key)}
                <div class="border border-gray-300 rounded-md">
                    <div class="bg-gray-100 px-3 py-1 font-semibold">{tower.label}</div>
                    <table class="w-full text-left text-sm">
                        <thead>
                            <tr class="text-gray-600">
                                <th class="px-3 py-1">Sector</th>
                                <th class="px-3 py-1">Cell ID</th>
                                <th class="px-3 py-1">EARFCN</th>
                                <th class="px-3 py-1">PCI</th>
                            </tr>
                        </thead>
                        <tbody>
                            {#each tower.cells as cell (`${cell.earfcn}/${cell.pci}`)}
                                <tr class="border-t border-gray-200">
                                    <td class="px-3 py-1">{cell.sector ?? '-'}</td>
                                    <td class="px-3 py-1">{cell.eci ?? '-'}</td>
                                    <td class="px-3 py-1">{cell.earfcn}</td>
                                    <td class="px-3 py-1">{cell.pci}</td>
                                </tr>
                            {/each}
                        </tbody>
                    </table>
                </div>
            {:else}
                <div>No cells heard yet. Start a recording and move around.</div>
            {/each}
        {/if}
    </div>
</Modal>
