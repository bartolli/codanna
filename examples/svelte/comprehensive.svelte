<!--
  Comprehensive Svelte 5 component for parser maturity assessment.

  Exercises both code paths and the template constructs the parser handles:
  - a module `<script module>` block (plain JS) -> JavaScript sub-parser
  - an instance `<script lang="ts">` block      -> TypeScript sub-parser
  - imports (value, default, and type-only)
  - functions, $state / $derived / $effect runes
  - {#snippet} definitions and {@render} usage
  - {#if} and {#each} template blocks
-->

<script module>
    // Module context: runs once per module (plain JavaScript on purpose, so the
    // JS sub-parser path is covered alongside the TS instance block below).
    import { formatDate } from './utils/date.js';

    export const COMPONENT_VERSION = '1.0.0';

    let instanceCount = 0;

    /** Track how many instances have been created. */
    export function nextInstanceId() {
        instanceCount += 1;
        return instanceCount;
    }

    export { formatDate };
</script>

<script lang="ts">
    // Instance context: TypeScript (the Svelte 5 default).
    import Counter from './Counter.svelte';
    import { fetchUsers } from './api/users.ts';
    import type { User } from './types/user.ts';

    interface PanelProps {
        title: string;
        max?: number;
    }

    let { title, max = 100 }: PanelProps = $props();

    // Runes: reactive state and derived values.
    let count = $state(0);
    let users = $state<User[]>([]);
    let doubled = $derived(count * 2);
    let isMaxed = $derived(count >= max);

    $effect(() => {
        console.log(`count changed to ${count}`);
    });

    function increment(): void {
        count += 1;
    }

    function reset(): void {
        count = 0;
    }

    async function loadUsers(): Promise<void> {
        users = await fetchUsers();
    }

    const greeting = (user: User): string => `Hello, ${user.name}`;
</script>

<section>
    <h1>{title} (v{COMPONENT_VERSION})</h1>

    <!-- snippet definition: emitted as a Function symbol -->
    {#snippet userRow(user: User)}
        <li>{greeting(user)}</li>
    {/snippet}

    <Counter {count} on:increment={increment} />

    <p>Doubled: {doubled}</p>

    {#if isMaxed}
        <strong>Maxed out!</strong>
    {:else}
        <button onclick={increment}>Increment</button>
        <button onclick={reset}>Reset</button>
    {/if}

    <ul>
        {#each users as user (user.id)}
            {@render userRow(user)}
        {/each}
    </ul>

    <button onclick={loadUsers}>Load users</button>
</section>
