// The demo's history. Every project, machine and conversation here is made
// up; none of it comes from a real index.
import type { Agent } from "../lib/types";

export interface DemoMessage {
  readonly role: "user" | "assistant" | "tool";
  readonly content: string;
  readonly call?: { readonly name: string; readonly input: unknown };
  /** Set on a tool's output: the position of the call it answers. */
  readonly answers?: number;
}
export interface DemoChat {
  readonly id: string;
  readonly title: string;
  readonly agent: Agent;
  readonly machine: string;
  readonly project: string;
  readonly repository: string;
  /** How long before the page was opened the chat last changed. */
  readonly daysAgo: number;
  readonly messages: readonly DemoMessage[];
}
export interface DemoRecord {
  readonly id: string;
  readonly kind: "decision" | "preference" | "fact";
  readonly statement: string;
  readonly rationale: string;
  readonly review: "candidate" | "established";
  readonly recordedBy: string;
  readonly project: string | null;
  readonly daysAgo: number;
  readonly alternatives?: readonly string[];
  readonly consequences?: readonly string[];
  /** The chat and the position of the message the record quotes. */
  readonly source: { readonly chat: string; readonly message: number; readonly quote: string };
}

export const localMachine = "laptop";

const user = (content: string): DemoMessage => ({ role: "user", content });
const agent = (content: string): DemoMessage => ({ role: "assistant", content });
/** A tool call and its output, as the two messages an agent's history holds. */
function tool(name: string, input: unknown, output: string): DemoMessage[] {
  return [
    { role: "assistant", content: "", call: { name, input } },
    { role: "tool", content: output, answers: -1 },
  ];
}

interface Project {
  readonly path: string;
  readonly repository: string;
}
const projects = {
  storefront: {
    path: "/home/sam/code/storefront",
    repository: "https://github.com/harbor-goods/storefront.git",
  },
  billing: {
    path: "/home/sam/code/billing-service",
    repository: "https://github.com/harbor-goods/billing-service.git",
  },
  mobile: {
    path: "/home/sam/code/mobile-app",
    repository: "https://github.com/harbor-goods/mobile-app.git",
  },
  infra: {
    path: "/home/sam/code/infra",
    repository: "https://github.com/harbor-goods/infra.git",
  },
} satisfies Record<string, Project>;

function chat(
  id: string,
  title: string,
  by: Agent,
  machine: string,
  project: Project,
  daysAgo: number,
  messages: readonly (DemoMessage | DemoMessage[])[],
): DemoChat {
  return {
    id,
    title,
    agent: by,
    machine,
    project: project.path,
    repository: project.repository,
    daysAgo,
    messages: messages
      .flat()
      .map((message, position) =>
        message.answers === undefined ? message : { ...message, answers: position - 1 },
      ),
  };
}

export const chats: readonly DemoChat[] = [
  chat(
    "sign-in-tokens",
    "Move sign-in to short-lived tokens",
    "claude-code",
    "laptop",
    projects.storefront,
    0.1,
    [
      user(
        "Sign-out is unreliable: people stay signed in on other devices for days. Should sessions live in a cookie, or should we switch to tokens?",
      ),
      agent(
        "The session cookie lasts thirty days and nothing on the server can end it early, which is why sign-out on one device doesn't reach the others.\n\nI'd switch to **access tokens that expire after fifteen minutes**, with a refresh token kept in a cookie that the page's scripts can't read. Signing out then deletes the refresh token on the server, and every device is out within fifteen minutes.\n\nThe alternative is to keep cookies and add a server-side session table. That fixes sign-out too, but the mobile app would still need its own scheme.",
      ),
      user("ok, go with the tokens. keep the mobile app on the same scheme"),
      tool(
        "Edit",
        {
          file_path: "src/auth/session.ts",
          old_string: "const SESSION_DAYS = 30;",
          new_string: "const ACCESS_TOKEN_MINUTES = 15;\nconst REFRESH_TOKEN_DAYS = 30;",
        },
        "The file src/auth/session.ts has been updated.",
      ),
      tool(
        "Bash",
        { command: "pnpm test src/auth", description: "Run the sign-in tests" },
        "✓ src/auth/session.test.ts (9 tests)\n✓ src/auth/refresh.test.ts (6 tests)\n\nTest Files  2 passed (2)\n     Tests  15 passed (15)",
      ),
      agent(
        "Done. Sign-in now issues a fifteen-minute access token and a thirty-day refresh token, and signing out removes the refresh token on the server.\n\n- `src/auth/session.ts` issues and checks the tokens\n- `src/auth/refresh.ts` is the new endpoint the web and mobile apps call\n- The fifteen tests for sign-in pass\n\nThe mobile app needs one change on its side: store the refresh token in the device keychain instead of the cookie jar.",
      ),
      user("nice. can you note that decision so the mobile work picks it up?"),
      agent(
        "Saved it as a decision for the storefront repository: sign-in uses fifteen-minute access tokens with a server-held refresh token, and the mobile app follows the same scheme.",
      ),
    ],
  ),
  chat(
    "flaky-checkout-test",
    "Fix the flaky checkout test",
    "codex",
    "workstation",
    projects.storefront,
    1.2,
    [
      user(
        "checkout.spec.ts fails about one run in ten on CI with 'expected total 42.00, got 0.00'. It never fails on my machine.",
      ),
      agent(
        "The test reads the order total as soon as the page loads, but the total is filled in after the cart request returns. On a slow CI machine the assertion sometimes runs first and sees the placeholder.",
      ),
      tool(
        "Bash",
        {
          command: "pnpm exec playwright test checkout.spec.ts --repeat-each 30",
          description: "Repeat the checkout test to reproduce the failure",
        },
        '  27 passed\n  3 failed\n    checkout.spec.ts:41 › shows the order total\n      Expected: "42.00"\n      Received: "0.00"',
      ),
      agent(
        "Reproduced: three failures in thirty runs. I changed the test to wait for the total to show the expected value instead of reading it once. Thirty more runs all pass.\n\nThis was a test problem, not a bug in checkout: a shopper always sees the right total once the cart loads.",
      ),
      user("great, commit it"),
      agent("Committed as “Wait for the cart before checking the order total”."),
    ],
  ),
  chat(
    "invoice-export-queue",
    "Choose a queue for invoice exports",
    "claude-code",
    "workstation",
    projects.billing,
    2.3,
    [
      user(
        "Month-end invoice exports time out when a customer has more than a few thousand invoices. We need to move them to a background job. What should we use for the queue?",
      ),
      agent(
        "Three realistic options for this service:\n\n| Option | For | Against |\n| --- | --- | --- |\n| A table in the existing Postgres database | Nothing new to run; jobs commit with the invoice data | Polling; fine at this volume |\n| Redis streams | Fast, already used for caching | Jobs can be lost if Redis restarts |\n| A hosted queue | Scales without thought | New dependency, new credentials, new bill |\n\nExports run a few hundred times a month, so volume is not the concern. Losing an export is. I'd use **a jobs table in Postgres**: a job is created in the same transaction that marks the export as requested, so it can't be lost or run twice.",
      ),
      user("agreed, postgres. what happens if a worker dies halfway through?"),
      agent(
        "Each job row has a `locked_until` time. A worker claims a job by setting it ten minutes ahead and extends it while it works. If the worker dies, the time passes and another worker picks the job up. Exports write to a temporary file and rename it at the end, so a half-finished export is never served.",
      ),
      user("good. write it up as a decision and start on the table"),
      agent(
        "Saved the decision, with Redis streams and a hosted queue listed as the alternatives we turned down. The migration for the `export_jobs` table is in `migrations/0042_export_jobs.sql`.",
      ),
    ],
  ),
  chat(
    "product-search-speed",
    "Speed up the product search page",
    "cursor",
    "laptop",
    projects.storefront,
    3.1,
    [
      user("the search page takes almost two seconds to show results. where is the time going?"),
      agent(
        "Most of it is one query. The page asks for the matching products and then, for each product, asks again for its stock level: one request for the list and one more for every row on the page.\n\nWith 48 products per page that is 49 queries, about 1.6 seconds in total.",
      ),
      user("can we get stock in the same query?"),
      agent(
        "Yes. I joined the stock table into the product query and removed the per-row lookup. The page now makes one query and returns in about 180 milliseconds on the same data.\n\nI also added an index on `stock(product_id)`, which the join needs to stay fast as the catalogue grows.",
      ),
      user("perfect"),
    ],
  ),
  chat(
    "offline-cart",
    "Offline cart for the mobile app",
    "cursor",
    "laptop",
    projects.mobile,
    4.4,
    [
      user(
        "People lose their cart when they go through a tunnel. I want the cart to keep working offline and sync when the connection is back.",
      ),
      agent(
        "The cart currently lives only on the server, so every change needs a connection. To work offline it has to be stored on the phone and synced.\n\nThe simplest sound design: keep the cart on the device as a list of changes (added, removed, quantity set), apply them locally at once, and send them to the server in order when a connection is available. The server stays the authority on price and stock.",
      ),
      user("what if the same account changes the cart on two phones while both are offline?"),
      agent(
        "Then both lists of changes arrive and are applied in the order the server receives them. Adding the same product twice gives a quantity of two, and a removal wins over an earlier quantity change. Nothing is lost silently: if an item sold out in the meantime, the app says so when it syncs.",
      ),
      user("ok. and it should use the same sign-in as the website"),
      agent(
        "It does. I checked the saved decision for the storefront: sign-in uses fifteen-minute access tokens with a refresh token, and the mobile app is meant to follow the same scheme. The sync code refreshes the token before sending queued changes, and keeps the refresh token in the device keychain.",
      ),
    ],
  ),
  chat(
    "staging-database-costs",
    "Cut staging database costs",
    "antigravity",
    "build-box",
    projects.infra,
    5.2,
    [
      user(
        "staging costs nearly as much as production. what can we cut without making it useless?",
      ),
      agent(
        "Staging runs the same database size as production, around the clock, and keeps thirty days of backups. Three changes would cut most of the cost:\n\n1. Use a database two sizes smaller. Staging holds a tenth of the data.\n2. Stop it overnight and at weekends, when nobody deploys.\n3. Keep seven days of backups instead of thirty.",
      ),
      user("do 1 and 3. not 2, the nightly tests need it"),
      tool(
        "Edit",
        {
          file_path: "environments/staging/database.tf",
          old_string: 'instance_class = "db.r6g.xlarge"\n  backup_retention_period = 30',
          new_string: 'instance_class = "db.r6g.large"\n  backup_retention_period = 7',
        },
        "The file environments/staging/database.tf has been updated.",
      ),
      agent(
        "Changed the staging database to the smaller size and seven days of backups. The plan shows only those two changes. I left the schedule alone so the nightly tests keep running.",
      ),
    ],
  ),
  chat(
    "rename-billing-tables",
    "Rename the billing tables",
    "codex",
    "laptop",
    projects.billing,
    6.5,
    [
      user(
        "The tables are still called `charges_v2` and `charges_v2_items` from the old migration. I want them to be `invoices` and `invoice_lines`. How do we do that without downtime?",
      ),
      agent(
        "Renaming in one step would break the running service the moment the migration applies. The safe order is:\n\n1. Rename the tables and, in the same migration, create views with the old names that point at the new tables.\n2. Deploy the service reading and writing the new names.\n3. Once nothing uses the old names, drop the views.\n\nThe views make both names work during the deploy, so there is no window where queries fail.",
      ),
      user("do steps 1 and 2. leave the views for now"),
      agent(
        "Done. The migration renames both tables and adds the two views, and the service now uses `invoices` and `invoice_lines` everywhere. The views are still in place; dropping them is a one-line migration when you are ready.",
      ),
    ],
  ),
  chat(
    "refund-rounding",
    "Handle currency rounding in refunds",
    "copilot",
    "laptop",
    projects.billing,
    9.3,
    [
      user(
        "A customer was refunded 33.34 three times on a 100.00 order split three ways. We gave back 100.02.",
      ),
      agent(
        "Each part refund is calculated as a third of the total and rounded up on its own, so the rounding error is added three times.\n\nThe fix is to work in whole cents and give the remainder to the last refund: 33.33, 33.33 and 33.34. The parts then always add up to the original amount.",
      ),
      user("yes. and add a test for it"),
      agent(
        "Refunds are now calculated in cents, with the last part taking whatever is left. I added tests for a three-way and a seven-way split, and for currencies with no minor unit such as yen.",
      ),
    ],
  ),
  chat(
    "notification-permissions",
    "Push notification permissions flow",
    "opencode",
    "workstation",
    projects.mobile,
    12.4,
    [
      user(
        "Only about a fifth of people allow notifications. We ask on first launch. Is there a better moment?",
      ),
      agent(
        "Asking on first launch is the worst moment: the person doesn't yet know what the notifications would be for, and on iOS a refusal can't be asked again from inside the app.\n\nA better moment is right after they place their first order, with our own screen first: “Want to know when your order ships?” Only if they say yes do we show the system prompt.",
      ),
      user("makes sense, do that"),
      agent(
        "The app no longer asks on launch. After the first order it shows the explanation screen, and the system prompt only follows a yes. If the person declines our screen we ask again after their next order, at most three times.",
      ),
    ],
  ),
  chat(
    "rotate-deploy-keys",
    "Rotate the deploy keys",
    "claude-code",
    "build-box",
    projects.infra,
    15.6,
    [
      user(
        "The deploy keys haven't been changed since the project started. Rotate them, and tell me how often we should do this.",
      ),
      agent(
        "There are four deploy keys: one per service repository. I can add the new keys, switch the pipelines to them and remove the old ones, one repository at a time so a mistake only affects one service.\n\nFor how often: every ninety days is a common policy and easy to automate. I'd also rotate whenever someone with access leaves.",
      ),
      user("go ahead. ninety days is fine"),
      agent(
        "All four keys are rotated. Each pipeline ran a deploy to staging with its new key before I removed the old one. I added a scheduled job that opens a reminder issue every ninety days.",
      ),
    ],
  ),
  chat(
    "slow-image-uploads",
    "Why are image uploads slow?",
    "claude-code",
    "laptop",
    projects.storefront,
    20.2,
    [
      user(
        "Sellers complain that uploading product photos takes ages. A 5 MB photo takes about twenty seconds.",
      ),
      agent(
        "The upload goes through the web server, which resizes the photo to five sizes before it answers. The seller waits for all five.\n\nTwo changes would fix it: upload straight to file storage from the browser, and resize in the background. The seller sees the photo as soon as the upload finishes, and the smaller sizes appear a few seconds later.",
      ),
      user("do the background resize first, that sounds like the bigger win"),
      agent(
        "Resizing now happens after the upload returns. The same 5 MB photo is accepted in about three seconds, and the five sizes are ready roughly four seconds after that. The product page shows the original until they are.",
      ),
    ],
  ),
  chat("dark-theme", "Add a dark theme", "opencode", "laptop", projects.storefront, 27.1, [
    user("add a dark theme to the storefront. it should follow the system setting by default"),
    agent(
      "The colours are written directly into about sixty components, so a theme can't be switched in one place yet. I'll first move them into named variables (background, surface, text, accent), then define a dark set of the same names.",
    ),
    user("fine. keep the brand orange the same in both"),
    agent(
      "Done. Every colour now comes from a variable, and the dark theme follows the system setting unless the shopper picks one in the footer. The brand orange is unchanged; I darkened only the surfaces around it so the contrast still passes.",
    ),
  ]),
  chat(
    "service-authentication",
    "Authentication between services",
    "claude-code",
    "workstation",
    projects.billing,
    33.5,
    [
      user(
        "The storefront calls the billing service with a shared password in a header. That password is in three repositories. What should we use instead?",
      ),
      agent(
        "A shared password can't say which service is calling, and changing it means deploying everything at once.\n\nI'd have each service sign its requests with its own key, and have billing check the signature against a list of services it accepts. Each key can then be rotated on its own, and the logs show who called.",
      ),
      user("is that overkill for two services?"),
      agent(
        "For two services it is slightly more than you need today. But the mobile backend will call billing next quarter, and adding a third holder of a shared password is where these setups usually go wrong. Signed requests cost about a day now.",
      ),
      user("ok, do it"),
      agent(
        "Billing now accepts signed requests from the storefront and rejects the old header. The shared password is removed from all three repositories.",
      ),
    ],
  ),
  chat(
    "checkout-outage-review",
    "Review of the checkout outage",
    "codex",
    "build-box",
    projects.infra,
    41.3,
    [
      user(
        "Checkout was down for forty minutes yesterday afternoon. Go through the logs and tell me what happened.",
      ),
      agent(
        "The sequence, from the logs:\n\n1. A deploy of the storefront started a database migration that added an index to the orders table.\n2. Building the index locked the table for writes.\n3. Checkout could not create orders, and requests queued until they timed out.\n4. The migration finished after thirty-eight minutes and checkout recovered by itself.\n\nNothing was lost, but no orders could be placed during that time.",
      ),
      user("how do we stop this happening again?"),
      agent(
        "Two things. Build indexes without locking the table, which Postgres supports. And have the pipeline refuse a migration that takes a write lock on a large table unless someone has marked it as reviewed.",
      ),
      user("add both. and remember that we never lock the orders table in a migration"),
      agent(
        "Both are in place: the index migration helper now builds without a write lock, and the pipeline check is on. I saved the rule about the orders table so it comes up the next time anyone writes a migration.",
      ),
    ],
  ),
];

export const records: readonly DemoRecord[] = [
  {
    id: "record-sign-in-tokens",
    kind: "decision",
    statement:
      "Sign-in uses fifteen-minute access tokens with a server-held refresh token. The mobile app follows the same scheme.",
    rationale: "Chosen so that signing out reaches every device within fifteen minutes.",
    review: "established",
    recordedBy: "Claude Code",
    project: projects.storefront.repository,
    daysAgo: 0.1,
    alternatives: ["Keep session cookies and add a server-side session table"],
    consequences: ["The mobile app stores its refresh token in the device keychain"],
    source: {
      chat: "sign-in-tokens",
      message: 2,
      quote: "ok, go with the tokens. keep the mobile app on the same scheme",
    },
  },
  {
    id: "record-export-queue",
    kind: "decision",
    statement: "Invoice exports run as background jobs kept in a Postgres table.",
    rationale:
      "A job is created in the same transaction as the export request, so it cannot be lost.",
    review: "established",
    recordedBy: "Claude Code",
    project: projects.billing.repository,
    daysAgo: 2.3,
    alternatives: ["Redis streams", "A hosted queue"],
    consequences: [
      "Workers claim a job for ten minutes at a time and extend the claim while working",
    ],
    source: { chat: "invoice-export-queue", message: 2, quote: "agreed, postgres." },
  },
  {
    id: "record-orders-lock",
    kind: "preference",
    statement: "Never take a write lock on the orders table in a migration.",
    rationale: "A locking index build stopped checkout for forty minutes.",
    review: "established",
    recordedBy: "Codex",
    project: projects.infra.repository,
    daysAgo: 41.3,
    source: {
      chat: "checkout-outage-review",
      message: 4,
      quote: "remember that we never lock the orders table in a migration",
    },
  },
  {
    id: "record-refund-cents",
    kind: "fact",
    statement: "Refunds are calculated in whole cents, and the last part takes the remainder.",
    rationale: "Rounding each part on its own refunded more than the order total.",
    review: "candidate",
    recordedBy: "GitHub Copilot",
    project: projects.billing.repository,
    daysAgo: 9.3,
    source: { chat: "refund-rounding", message: 2, quote: "yes. and add a test for it" },
  },
  {
    id: "record-key-rotation",
    kind: "decision",
    statement:
      "Deploy keys are rotated every ninety days, and whenever someone with access leaves.",
    rationale: "The keys had not been changed since the project started.",
    review: "established",
    recordedBy: "Claude Code",
    project: projects.infra.repository,
    daysAgo: 15.6,
    alternatives: [],
    consequences: ["A scheduled job opens a reminder issue every ninety days"],
    source: { chat: "rotate-deploy-keys", message: 2, quote: "go ahead. ninety days is fine" },
  },
  {
    id: "record-brand-orange",
    kind: "preference",
    statement: "The brand orange stays the same in the light and dark themes.",
    rationale: "Asked for when the dark theme was added.",
    review: "candidate",
    recordedBy: "OpenCode",
    project: projects.storefront.repository,
    daysAgo: 27.1,
    source: {
      chat: "dark-theme",
      message: 2,
      quote: "keep the brand orange the same in both",
    },
  },
];
