<script lang="ts">
    import {
        get_config,
        get_wifi_status,
        scan_wifi_networks,
        set_config,
        type WifiNetwork,
        type WifiStatus,
    } from '$lib/utils.svelte';
    import Modal from './Modal.svelte';

    let { shown = $bindable() }: { shown: boolean } = $props();

    let status: WifiStatus | null = $state(null);
    let networks: WifiNetwork[] = $state([]);
    let scanning = $state(false);
    let scanned = $state(false);
    let scan_error = $state('');
    let selected: WifiNetwork | null = $state(null);
    let password = $state('');
    let connecting = $state(false);
    let message = $state('');
    let message_is_error = $state(false);

    // Poll the connection status only while the panel is open.
    $effect(() => {
        if (!shown) {
            return;
        }
        const read = async () => {
            try {
                status = await get_wifi_status();
            } catch {
                status = null;
            }
        };
        read();
        const interval = setInterval(read, 3000);
        return () => clearInterval(interval);
    });

    async function scan() {
        scanning = true;
        scan_error = '';
        message = '';
        try {
            const found = await scan_wifi_networks();
            // Strongest first; one row per network name (keep the strongest).
            const best = new Map<string, WifiNetwork>();
            for (const n of found) {
                if (!n.ssid) continue;
                const have = best.get(n.ssid);
                if (!have || n.signal_dbm > have.signal_dbm) best.set(n.ssid, n);
            }
            networks = [...best.values()].sort((a, b) => b.signal_dbm - a.signal_dbm);
            scanned = true;
        } catch (e) {
            networks = [];
            scan_error = `Scan failed: ${e instanceof Error ? e.message : e}`;
        } finally {
            scanning = false;
        }
    }

    function bars(dbm: number): number {
        if (dbm >= -55) return 4;
        if (dbm >= -65) return 3;
        if (dbm >= -75) return 2;
        return 1;
    }

    function is_open(n: WifiNetwork): boolean {
        const s = n.security.toLowerCase();
        return s === '' || s.includes('open') || s === 'none';
    }

    function pick(n: WifiNetwork) {
        selected = n;
        password = '';
        message = '';
    }

    async function connect() {
        if (!selected) return;
        connecting = true;
        message = '';
        message_is_error = false;
        try {
            const config = await get_config();
            config.wifi_enabled = true;
            config.wifi_ssid = selected.ssid;
            config.wifi_password = password;
            config.wifi_security =
                selected.security === 'WPA3' || selected.security === 'WPA3 (transition)'
                    ? 'sae'
                    : 'wpa_psk';
            await set_config(config);
            message = `Saved. Connecting to "${selected.ssid}"... Rayhunter restarts to apply this, so the page may pause for a few seconds.`;
            selected = null;
            password = '';
        } catch (e) {
            message_is_error = true;
            message = `Couldn't save: ${e instanceof Error ? e.message : e}`;
        } finally {
            connecting = false;
        }
    }
</script>

<Modal bind:shown title="Wi-Fi">
    <div class="flex flex-col gap-3 overflow-auto p-1">
        <div class="border border-gray-300 rounded-md px-3 py-2">
            {#if status === null}
                <span class="text-gray-600">Wi-Fi client status unavailable.</span>
            {:else if status.state === 'connected'}
                <span class="text-green-700 font-semibold">Connected</span>
                to "{status.ssid}" ({status.ip})
            {:else if status.state === 'connecting'}
                <span class="font-semibold">Connecting...</span>
            {:else if status.state === 'failed'}
                <span class="text-red-700 font-semibold">Failed:</span>
                {status.error}
            {:else}
                <span class="text-gray-600">Not connected ({status.state}).</span>
            {/if}
        </div>

        <div class="flex flex-row items-center gap-3">
            <button
                type="button"
                class="px-4 py-2 bg-blue-500 hover:bg-blue-700 text-white rounded-md disabled:opacity-50"
                disabled={scanning}
                onclick={scan}
            >
                {scanning ? 'Scanning...' : scanned ? 'Scan again' : 'Scan for networks'}
            </button>
            {#if scanned && !scanning}
                <span class="text-sm text-gray-600">{networks.length} networks found</span>
            {/if}
        </div>

        {#if scan_error}
            <div class="text-red-700">{scan_error}</div>
        {/if}

        {#if selected}
            <div class="border-2 border-green-500 rounded-md p-3 flex flex-col gap-2">
                <div class="font-semibold">{selected.ssid}</div>
                <div class="text-sm text-gray-600">
                    {selected.security} &middot; {selected.signal_dbm} dBm
                </div>
                {#if is_open(selected)}
                    <div class="text-sm text-red-700">
                        Open networks aren't supported by Rayhunter's Wi-Fi client.
                    </div>
                {:else}
                    <input
                        type="password"
                        class="form-control"
                        placeholder="Password"
                        autocomplete="off"
                        bind:value={password}
                    />
                    <div class="flex flex-row gap-2">
                        <button
                            type="button"
                            class="px-4 py-2 bg-blue-500 hover:bg-blue-700 text-white rounded-md disabled:opacity-50"
                            disabled={connecting || password.length < 8}
                            onclick={connect}
                        >
                            {connecting ? 'Saving...' : 'Connect'}
                        </button>
                        <button
                            type="button"
                            class="px-4 py-2 border border-gray-300 rounded-md"
                            onclick={() => (selected = null)}>Cancel</button
                        >
                    </div>
                    {#if password.length > 0 && password.length < 8}
                        <div class="text-xs text-gray-600">WPA passwords are at least 8 characters.</div>
                    {/if}
                {/if}
            </div>
        {/if}

        {#if message}
            <div class={message_is_error ? 'text-red-700' : 'text-green-700'}>{message}</div>
        {/if}

        {#if networks.length > 0}
            <div class="border border-gray-300 rounded-md divide-y divide-gray-200">
                {#each networks as network (network.ssid)}
                    <button
                        type="button"
                        class="w-full px-3 py-3 text-left flex flex-row items-center gap-3 hover:bg-gray-50"
                        onclick={() => pick(network)}
                    >
                        <span
                            class="flex flex-row items-end gap-0.5 h-5 shrink-0"
                            aria-label="{network.signal_dbm} dBm"
                        >
                            {#each [1, 2, 3, 4] as level}
                                <span
                                    class="w-1 rounded-sm {level <= bars(network.signal_dbm)
                                        ? 'bg-green-500'
                                        : 'bg-gray-300'}"
                                    style="height:{level * 25}%"
                                ></span>
                            {/each}
                        </span>
                        <span class="flex-1 truncate font-medium">{network.ssid}</span>
                        <span class="text-sm text-gray-500 shrink-0"
                            >{network.signal_dbm} dBm &middot; {network.security}</span
                        >
                    </button>
                {/each}
            </div>
        {:else if scanned && !scanning && !scan_error}
            <div class="text-gray-600">No networks found. Move closer and scan again.</div>
        {/if}
    </div>
</Modal>
