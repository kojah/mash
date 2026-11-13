// The conversation with Mashy. It belongs to this tab: it survives moving
// between pages, is never saved, and is not part of the indexed history.
import { type AssistantEvent, ask, type OpenChat, type Turn } from "./assistant";

export interface Note {
  readonly kind: "partial" | "error";
  readonly title: string;
  readonly text: string;
  /** Whether asking again could help. */
  readonly retry: boolean;
}

const titles: Record<string, string> = {
  unreachable: "Couldn’t reach the model",
  refused: "The model’s service refused the key",
  limited: "The model’s service is busy",
  failed: "Mashy couldn’t answer",
};

class Conversation {
  open = $state(false);
  turns = $state<readonly Turn[]>([]);
  /** The answer being written, before it joins `turns`. */
  answer = $state("");
  phase = $state<"idle" | "searching" | "answering">("idle");
  note = $state<Note | null>(null);
  /** An answer arrived while the chat was closed. */
  unread = $state(false);
  private controller: AbortController | undefined;
  private context: OpenChat | undefined;

  get busy(): boolean {
    return this.phase !== "idle";
  }

  toggle(open = !this.open): void {
    this.open = open;
    if (open) this.unread = false;
  }

  /** Asks `question`, with the chat on screen if there is one. */
  async ask(question: string, openChat: OpenChat | undefined): Promise<void> {
    const text = question.trim();
    if (!text || this.busy) return;
    this.turns = [...this.turns, { role: "user", content: text }];
    this.context = openChat;
    await this.answerLast();
  }

  /** Asks the last question again after a failure. */
  async retry(): Promise<void> {
    if (this.busy || this.turns.at(-1)?.role !== "user") return;
    await this.answerLast();
  }

  private async answerLast(): Promise<void> {
    this.note = null;
    this.answer = "";
    this.phase = "searching";
    const controller = new AbortController();
    this.controller = controller;
    let finished = false;
    const heard = (event: AssistantEvent): void => {
      if (event.type === "searching") this.phase = "searching";
      else if (event.type === "text") {
        this.phase = "answering";
        this.answer += event.delta;
      } else if (event.type === "done") {
        finished = true;
        if (event.partial)
          this.note = {
            kind: "partial",
            title: "There is more than I could read in one go",
            text: "Ask about a narrower period or topic to go deeper.",
            retry: false,
          };
      } else {
        finished = true;
        this.note = {
          kind: "error",
          title: titles[event.reason] ?? "Mashy couldn’t answer",
          text: event.message,
          retry: event.reason !== "refused",
        };
      }
    };
    try {
      await ask(this.turns, this.context, controller.signal, heard);
      if (!finished && !this.note)
        this.note = {
          kind: "error",
          title: "The answer was cut short",
          text: "The connection to Mash closed before Mashy finished.",
          retry: true,
        };
    } catch (error) {
      if (!controller.signal.aborted)
        this.note = {
          kind: "error",
          title: "Mashy couldn’t answer",
          text: error instanceof Error ? error.message : "The request failed.",
          retry: true,
        };
    } finally {
      // Whatever was written is kept, including an answer the reader stopped.
      if (this.answer) {
        this.turns = [...this.turns, { role: "assistant", content: this.answer }];
        if (!this.open) this.unread = true;
      }
      this.answer = "";
      this.phase = "idle";
      this.controller = undefined;
    }
  }

  stop(): void {
    this.controller?.abort();
  }

  clear(): void {
    this.stop();
    this.turns = [];
    this.note = null;
  }
}

export const mashy = new Conversation();
