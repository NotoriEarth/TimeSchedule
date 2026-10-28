<script lang="ts">
  let { name, size = 20, stroke = 1.7 }: { name: string; size?: number; stroke?: number } = $props();
  const paths: Record<string, string> = {
    calendar: 'M8 2v4m8-4v4M3 10h18M5 4h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Zm2 10h2m4 0h2m-8 4h2',
    check: 'm5 12 4 4L19 6',
    tasks: 'M9 5h11M9 12h11M9 19h11M3 5h.01M3 12h.01M3 19h.01',
    note: 'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Zm0 0v6h6M8 13h8M8 17h5',
    plus: 'M12 5v14M5 12h14',
    left: 'm15 18-6-6 6-6',
    right: 'm9 18 6-6-6-6',
    close: 'm6 6 12 12M6 18 18 6',
    arrow: 'M7 17 17 7M7 7h10v10',
    monitor: 'M4 3h16a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1Zm4 18h8m-4-4v4',
    clock: 'M12 8v4l3 2M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0',
    trash: 'M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7',
    leaf: 'M20 3C10 2 4 6 4 12a7 7 0 0 0 7 7c6 0 10-6 9-16ZM3 22 15 10',
    lock: 'M7 10V7a5 5 0 0 1 10 0v3M6 10h12a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1v-9a1 1 0 0 1 1-1Zm6 4v3',
    unlock: 'M7 10V7a5 5 0 0 1 9-3M6 10h12a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1v-9a1 1 0 0 1 1-1Zm6 4v3',
    move: 'M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3',
    grip: 'M8 5h.01M16 5h.01M8 12h.01M16 12h.01M8 19h.01M16 19h.01',
    sun: 'M12 2v2m0 16v2M2 12h2m16 0h2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4M17 12a5 5 0 1 1-10 0 5 5 0 0 1 10 0',
    refresh: 'M20 7v5h-5M4 17v-5h5M6 6a8 8 0 0 1 13 3M18 18A8 8 0 0 1 5 15',
  };
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width={stroke} stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={paths[name] ?? paths.calendar} /></svg>
