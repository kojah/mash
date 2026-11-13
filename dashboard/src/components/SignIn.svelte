<script lang="ts">
import KeyRound from "@lucide/svelte/icons/key-round";
import { signIn } from "../lib/session.svelte";

const { done }: { done: () => void } = $props();

let token = $state("");
let busy = $state(false);
let problem = $state<string | undefined>(undefined);

async function submit(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  if (busy || token.trim() === "") return;
  busy = true;
  problem = await signIn(token.trim());
  busy = false;
  if (problem === undefined) {
    token = "";
    done();
  }
}
</script>

<main class="signin">
  <form class="state-c" onsubmit={submit}>
    <h1 class="h1"><KeyRound size={18} aria-hidden="true" /> Sign in to Mash</h1>
    <p class="meta">
      This dashboard can be reached from other devices, so it asks you to sign in. Enter a one-time code from the
      machine that runs it, or its access token.
    </p>
    <label class="meta" for="signin-token">Code or access token</label>
    <!-- svelte-ignore a11y_autofocus: the token is the only thing to do on this screen. -->
    <input
      id="signin-token"
      class="field"
      type="text"
      autocomplete="one-time-code"
      autocapitalize="off"
      autocorrect="off"
      spellcheck="false"
      autofocus
      bind:value={token}
      aria-describedby={problem === undefined ? undefined : "signin-problem"}
      aria-invalid={problem !== undefined}
    />
    {#if problem !== undefined}<p id="signin-problem" class="signin-problem" role="alert">{problem}</p>{/if}
    <button class="btn p" type="submit" disabled={busy || token.trim() === ""}>{busy ? "Signing in…" : "Sign in"}</button>
  </form>
</main>
