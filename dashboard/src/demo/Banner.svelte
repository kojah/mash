<script lang="ts">
// Says plainly that this is a demonstration, on every page of it.
</script>

<aside class="demo" aria-label="About this demo">
  <span><b>Demo.</b> This is a build of Mash with synthetic chat data.</span>
  <a href="https://getmash.io/quick-start/">Install Mash</a>
</aside>

<style>
  .demo {
    position: fixed;
    inset: auto 0 0;
    z-index: 50;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: 2px 14px;
    min-height: 40px;
    padding: 6px 14px;
    background: var(--cream);
    color: var(--cream-ink);
    font-family: var(--sans);
    font-size: 14px;
    text-align: center;
  }
  .demo a {
    color: inherit;
    font-weight: 600;
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  /* The page and the rail end above the bar. */
  :global(body) {
    padding-bottom: 40px;
  }
  :global(#rail) {
    height: calc(100vh - 40px);
  }
</style>
