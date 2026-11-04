export const meta = {
  name: 'build',
  description: 'Plan (per-task light/heavy) -> build in worktrees -> heavy tasks: blind review/fix on a pinned commit, then check against a written checklist -> merge to main locally -> once everything is ready: update Obsidian memory + README, push -> after the push: remove this run\'s merged worktrees and branches',
  whenToUse: 'Every code change. args: a request string, or {request, defer: true} to build now and verify heavy tasks later, or {verify: [...]} for the deferred review/check pass',
  phases: [
    { title: 'Plan', detail: 'split into tasks, mark each light or heavy, write a checklist for heavy ones', model: 'opus' },
    { title: 'Build', detail: 'one builder per task, in its own worktree', model: 'sonnet' },
    { title: 'Review', detail: 'heavy only: blind review (Opus) of a pinned commit; fixes land in the builder worktree (Sonnet first, Opus from round 2)', model: 'opus' },
    { title: 'Check', detail: 'heavy only: tests + the written checklist; a failed item goes back to a fix (Opus), one more review, then the check again (2 checks at most)', model: 'sonnet' },
    { title: 'Ship', detail: 'merge passing tasks into local main, test once; if main is red: fix (Opus), review, test again (2 fixes at most); no push', model: 'sonnet' },
    { title: 'Finish', detail: 'only when everything is ready: update Obsidian memory + README, commit, push main', model: 'sonnet' },
    { title: 'Clean', detail: 'only after Finish pushed: remove this run\'s merged worktrees and branches, never forced', model: 'sonnet' },
  ],
}

const OPUS = 'opus'
const SONNET = 'sonnet'
const MAX_TASKS = 4
const FIX_ROUNDS = 3
// Checks per heavy task: a failed check gets one fix + review + check again
const CHECK_ROUNDS = 2
// Fixes when main is red after Ship merged the tasks: fix + review + test again
const SHIP_FIX_ROUNDS = 2

// First fix round is Sonnet; if the reviewer still says FIX, later rounds escalate to Opus
const fixModel = attempt => (attempt === 0 ? SONNET : OPUS)

const PLAN = {
  type: 'object',
  properties: {
    reason: { type: 'string', description: 'one sentence on how the work was split and weighed' },
    tasks: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          id: { type: 'string', description: 'short kebab-case id, used as branch name' },
          weight: { type: 'string', enum: ['light', 'heavy'] },
          brief: { type: 'string', description: 'what to build, acceptance criteria, files likely touched. Notes and docs for this change belong in this same task.' },
          checklist: {
            type: 'array',
            items: { type: 'string' },
            description: 'heavy tasks only: concrete things a checker must verify (what to open or run, what to compare with the design, what is off limits)',
          },
        },
        required: ['id', 'weight', 'brief'],
      },
    },
  },
  required: ['reason', 'tasks'],
}

const BUILD = {
  type: 'object',
  properties: {
    branch: { type: 'string' },
    sha: { type: 'string', description: 'git rev-parse HEAD of the final commit on the branch' },
    worktree: { type: 'string', description: 'absolute path of this worktree (git rev-parse --show-toplevel)' },
    summary: { type: 'string' },
  },
  required: ['branch', 'sha', 'worktree', 'summary'],
}

const REVIEW = {
  type: 'object',
  properties: {
    reviewed_sha: { type: 'string', description: 'the commit you actually reviewed (git rev-parse HEAD or the end of the range)' },
    verdict: { type: 'string', enum: ['SHIP', 'FIX'] },
    findings: { type: 'array', items: { type: 'string' } },
  },
  required: ['reviewed_sha', 'verdict', 'findings'],
}

const FIX = {
  type: 'object',
  properties: {
    sha: { type: 'string', description: 'git rev-parse HEAD after your commit (unchanged if you committed nothing)' },
    branch: { type: 'string' },
    worktree: { type: 'string' },
    summary: { type: 'string' },
  },
  required: ['sha', 'summary'],
}

const CHECK = {
  type: 'object',
  properties: {
    items: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          item: { type: 'string' },
          status: { type: 'string', enum: ['pass', 'fail', 'cannot_test'] },
          note: { type: 'string' },
        },
        required: ['item', 'status'],
      },
    },
    notes: { type: 'string' },
    worktree: { type: 'string', description: 'absolute path of the worktree you checked in (git rev-parse --show-toplevel)' },
  },
  required: ['items', 'notes'],
}

const SHIP = {
  type: 'object',
  properties: {
    merged: { type: 'array', items: { type: 'string' }, description: 'branches merged into main' },
    base: { type: 'string', description: 'main sha before the first merge' },
    head: { type: 'string', description: 'main sha after the last merge' },
    tests_passed: { type: 'boolean' },
    notes: { type: 'string', description: 'skipped branches, test failures, anything the main session must know' },
  },
  required: ['merged', 'base', 'head', 'tests_passed', 'notes'],
}

const RETEST = {
  type: 'object',
  properties: {
    head: { type: 'string', description: 'git rev-parse HEAD on main' },
    tests_passed: { type: 'boolean' },
    notes: { type: 'string', description: 'the failing tests with their errors, or that everything passed' },
  },
  required: ['head', 'tests_passed', 'notes'],
}

const FINISH = {
  type: 'object',
  properties: {
    memory_updated: { type: 'boolean', description: 'docs/memory/ changed' },
    readme_updated: { type: 'boolean', description: 'README.md changed' },
    docs_sha: { type: 'string', description: 'sha of the docs commit, empty if nothing changed' },
    pushed: { type: 'boolean' },
    head: { type: 'string', description: 'main sha that was pushed (or would have been)' },
    notes: { type: 'string', description: 'what was written, why a doc needed no change, or why the push did not happen' },
  },
  required: ['memory_updated', 'readme_updated', 'docs_sha', 'pushed', 'head', 'notes'],
}

const CLEAN = {
  type: 'object',
  properties: {
    removed: {
      type: 'array',
      items: {
        type: 'object',
        properties: { worktree: { type: 'string' }, size: { type: 'string', description: 'folder size before removal, e.g. 2.3G' } },
        required: ['worktree'],
      },
    },
    deleted_branches: { type: 'array', items: { type: 'string' } },
    skipped: {
      type: 'array',
      items: {
        type: 'object',
        properties: { worktree: { type: 'string' }, branch: { type: 'string' }, reason: { type: 'string' } },
        required: ['reason'],
      },
    },
    freed: { type: 'string', description: 'rough total space freed, e.g. ~28 GB' },
    notes: { type: 'string' },
  },
  required: ['removed', 'deleted_branches', 'skipped', 'freed', 'notes'],
}

// Rules every agent follows (see AGENTS.md)
const TOOLS =
  `\n\nTool rules:\n` +
  `- Shell commands go through RTK (a Bash hook rewrites them). Prefer rtk forms like "rtk git", "rtk test", "rtk grep"; use "rtk bypass <cmd>" only if output is unusable.\n` +
  `- For any library, framework, SDK, CLI or API: look up current docs with the Context7 MCP (resolve-library-id, then query-docs), matching the version the project uses. Do not rely on memory.\n` +
  `- Project memory is an Obsidian vault at docs/memory/ (start at docs/memory/index.md). Read the notes relevant to your work before starting.\n` +
  `- Never do anything that could interrupt the owner's other apps (media playback, calls, running builds). For visuals use the project's own demo or debug modes if it has any, never anything that touches real user data.\n` +
  `- Never push to origin unless you are the Finish step, never force-push, and never delete branches or worktrees unless you are the Clean step. Clean removes them once Finish has pushed main, and never forces it (no --force, no -D).\n` +
  `- Proof before claims: never report something as passing, fixed or working unless you ran the command in this session and saw its output.\n` +
  `- Nobody can answer questions during this workflow. Do not stop to ask; decide from the brief and report assumptions in your summary or notes.`

// Fixers find the cause before changing code
const FIX_RULES =
  `\nFor each finding: first reproduce it (a failing test where possible) and find the root cause, then fix the cause, not the symptom. ` +
  `If you believe a finding is wrong, do not change code for it; explain why in your summary.`

if (!args) throw new Error('Pass the request as args')
const input = typeof args === 'string' ? { request: args } : args
const incomplete = []

// agent() that never throws: a failed or skipped agent becomes null plus a note
async function safe(label, prompt, opts) {
  try {
    const out = await agent(prompt, { label, ...opts })
    if (out == null) incomplete.push(`${label}: no result`)
    return out
  } catch (e) {
    incomplete.push(`${label}: ${e && e.message ? e.message : e}`)
    return null
  }
}

function reviewPrompt(subject, brief, sha) {
  return (
    `You are a blind reviewer. ${subject}. Do not read any builder notes. Check correctness against the brief AND try to break it with edge cases (empty input, errors, boundaries, old data and config files). ` +
    `First run git rev-parse on the end of what you review and return it as reviewed_sha; it must be ${sha}. Return SHIP only if there is nothing worth fixing.\n\nBrief:\n${brief}`
  )
}

async function reviewAt(label, subject, brief, sha) {
  for (let attempt = 1; attempt <= 2; attempt++) {
    const r = await safe(label, reviewPrompt(subject, brief, sha) + TOOLS, { phase: 'Review', model: OPUS, effort: 'high', schema: REVIEW })
    if (!r) return null
    if (r.reviewed_sha && r.reviewed_sha.startsWith(sha.slice(0, 7))) return r
    log(`${label}: reviewer looked at ${r.reviewed_sha}, not ${sha}; asking again`)
  }
  incomplete.push(`${label}: the reviewer never looked at ${sha}`)
  return null
}

function checkPrompt(where, sha, task) {
  const list = (task.checklist && task.checklist.length ? task.checklist : ['The feature in the brief works as described']).map((c, i) => `${i + 1}. ${c}`).join('\n')
  return (
    `${where}\nConfirm that git rev-parse HEAD is ${sha}; if not, say so in notes and stop. Do not modify tracked files and do not commit.\n` +
    `Run the full test suite (the project's test command, see 'Project commands' in AGENTS.md) and the project's build command. Then go through this checklist item by item and report each as pass, fail, or cannot_test. ` +
    `Use cannot_test (with the reason) for anything that needs an action you must not take or a setup you do not have (real third-party accounts, real hardware, other apps' playback, anything that needs a human), instead of guessing. Use the project's demo or debug modes for visuals and compare with the design files named in the brief.\n\nChecklist:\n${list}\n\nBrief:\n${task.brief}` +
    TOOLS
  )
}

function checkResult(c) {
  if (!c) return { passed: false, skipped: false, items: [], notes: 'the check agent failed' }
  const failed = c.items.filter(i => i.status === 'fail')
  return { passed: failed.length === 0, skipped: false, items: c.items, notes: c.notes }
}

async function ship(label, ready, request) {
  return safe(
    label,
    `Ship these finished task branches to main, in this order: ${ready.map(r => r.branch).join(', ')}.\n` +
    `1. If the working tree on main has uncommitted changes (other than the untracked .claude/worktrees/ folder), STOP and report; do not stash or discard them.\n` +
    `2. Check out main. Note its sha as base. Merge each branch into main with --no-ff. If a merge conflict is not trivial, abort that merge, skip the branch, and report it. A conflict in docs/memory/index.md is trivial: keep both sides' lines.\n` +
    `3. Run the full test suite once on main (the project's test command, see 'Project commands' in AGENTS.md). If it fails, report the failures. Note main's sha as head.\n` +
    `4. Do NOT update docs and do NOT push; the Finish step does both once everything is ready. Do not delete branches or worktrees; the Clean step does that after the push.\n\n` +
    `Request: ${request}\nTasks:\n${ready.map(r => `- ${r.id}: ${r.summary}`).join('\n')}` + TOOLS,
    { phase: 'Ship', model: SONNET, effort: 'medium', schema: SHIP }
  )
}

// Ship, then if the merged tasks break main together: a fixer (Opus) commits on main, a blind review of that fix,
// and an independent test run. Up to SHIP_FIX_ROUNDS fixes; main stays red (no Finish, no push) if they fail.
async function shipWithFixes(label, ready, request) {
  const shipped = await ship(label, ready, request)
  if (!shipped || shipped.tests_passed || !shipped.merged.length) return shipped
  const tasks = ready.map(r => `- ${r.id}: ${r.summary}`).join('\n')
  let head = shipped.head
  let failures = shipped.notes
  for (let round = 1; round <= SHIP_FIX_ROUNDS; round++) {
    log(`${label}: main is red after the merge; fix round ${round}`)
    const fix = await safe(
      `${label}:fix#${round}`,
      `The full test suite fails on local main after these task branches were merged into it. Each passed its own tests, so look first at how the merged work breaks together.\n` +
      `Work in the main repository on branch main: no worktree, no new branch. Confirm git rev-parse HEAD is ${head} and the working tree is clean (the untracked .claude/worktrees/ folder is fine); if not, STOP, commit nothing and say why in summary.\n` +
      `Fix the failures, run the full test suite (see 'Project commands' in AGENTS.md), and commit on main as "fix: make main green after merge". Return the new sha (git rev-parse HEAD). Do not push.\n\nFailures:\n${failures}` +
      FIX_RULES + `\n\nRequest: ${request}\nTasks:\n${tasks}` + TOOLS,
      { phase: 'Ship', model: OPUS, effort: 'high', schema: FIX }
    )
    if (!fix || !fix.sha || fix.sha === head) { log(`${label}: the fix for red main produced no new commit`); break }
    head = fix.sha
    const review = await reviewAt(
      `review:${label}:fix#${round}`,
      `You get the request and the diff git diff ${shipped.head}..${head} on main (in the main repository): a fix for tests that failed after merging the tasks below`,
      `Request: ${request}\nTasks:\n${tasks}\nTest failures the fix must resolve without weakening or skipping tests:\n${failures}`,
      head
    )
    if (!review || review.verdict !== 'SHIP') {
      log(`${label}: the fix for red main did not pass review`)
      return { ...shipped, head, tests_passed: false, notes: `${shipped.notes}\nMain was red after the merge; fix ${head} is on local main but did not pass review: ${review ? review.findings.join('; ') : 'the review did not finish'}` }
    }
    const t = await safe(
      `${label}:retest#${round}`,
      `Work in the main repository on branch main. Confirm git rev-parse HEAD is ${head}; if not, report tests_passed=false and say so in notes. Run the full test suite once (see 'Project commands' in AGENTS.md). Do not modify files, do not commit, do not push. Report every failing test with its error.` + TOOLS,
      { phase: 'Ship', model: SONNET, effort: 'medium', schema: RETEST }
    )
    if (!t) break
    if (t.tests_passed && t.head && t.head.startsWith(head.slice(0, 7))) {
      return { ...shipped, head, tests_passed: true, notes: `${shipped.notes}\nMain was red after the merge; fixed on main in ${round} round(s) (${head}): ${fix.summary}` }
    }
    failures = t.notes
  }
  return { ...shipped, head, tests_passed: false, notes: `${shipped.notes}\nMain is still red after the fixes (local main at ${head}): ${failures}` }
}

// Runs only when every task is merged, reviewed/checked where required, and main is green
async function finish(label, request, head, lines) {
  return safe(
    label,
    `You are the Finish step. All work for this request is merged into local main, reviewed, checked and tested.\n` +
    `1. Work in the main repository on branch main. Confirm ${head ? `git rev-parse HEAD is ${head}` : 'HEAD is main'} and the working tree is clean (the untracked .claude/worktrees/ folder is fine). If not, STOP: do not commit or push, and say why in notes.\n` +
    `2. Obsidian memory (docs/memory/, index docs/memory/index.md): read the index, then add or update one note per decision, gotcha or convention this work introduced. Frontmatter with tags and today's date, [[wikilinks]] to related notes, a link from the index. Update an existing note instead of duplicating it, and don't record what the code or git history already says. If nothing is worth keeping, write nothing and say so.\n` +
    `3. README.md: update every part this work made out of date or incomplete (features, usage, setup, commands, config, screenshots list). Keep its existing structure and tone. If no part is affected, leave it and say why.\n` +
    `4. If anything changed, commit only those docs as "docs: update memory and README" and report the sha. Do not touch code.\n` +
    `5. Push main to origin (git push origin main). Never force-push. If the push is rejected, do not pull, rebase or retry: report it with pushed=false.\n\n` +
    `Request: ${request}\nWhat was done:\n${lines.join('\n')}` + TOOLS,
    { phase: 'Finish', model: SONNET, effort: 'medium', schema: FINISH }
  )
}

// Runs only after Finish pushed main. items: [{branch, worktree}] this run created; either may be null
async function clean(label, items) {
  const list = items.map(i => `- worktree: ${i.worktree || 'none'}; branch: ${i.branch || 'none'}`).join('\n')
  return safe(
    label,
    `You are the Clean step. Finish has pushed main, so the work below is on origin. Remove what this run left behind, and nothing else.\n` +
    `Work in the main repository on branch main, never inside one of the worktrees below. Touch only the worktrees and branches listed here (plus each worktree's own worktree-* branch, see 2), never other worktrees, branches or folders: other sessions may be using them.\n` +
    `1. Worktree (if given): skip it, with the reason, if git worktree list does not show it, if git -C <path> status --porcelain shows changes, or if its HEAD is not in main (git merge-base --is-ancestor <its HEAD> main). Otherwise note the folder size (du -sh <path>). If a folder inside the worktree is a link to a folder outside it (a Windows junction or a symlink, e.g. a shared node_modules), remove only the link first (Windows: cmd /c rmdir "<link>", with MSYS_NO_PATHCONV=1 in Git Bash; elsewhere: rm "<link>" without -r) and never delete it recursively: git worktree remove follows a Windows junction and deletes the folder it points to. Then git worktree remove <path>. Never use --force. If it refuses only because of untracked build output (target/, node_modules/, dist/ and the like), delete that folder and try once more. If a file is locked by a running process, skip it and say so.\n` +
    `2. Branches: the branch listed, and the worktree tool's own branch for that worktree (worktree-<folder name>, if it exists). Delete each with git branch -d, never -D. If git says it is not fully merged, or it is still checked out somewhere, keep it and report why.\n` +
    `3. Run git worktree prune.\n` +
    `Report every removed worktree with its size, every deleted branch, everything skipped (worktree, branch, reason), and the rough total freed. Do not commit and do not push.\n\n` +
    `Items:\n${list}` + TOOLS,
    { phase: 'Clean', model: SONNET, effort: 'medium', schema: CLEAN }
  )
}

// Check against the written checklist. A failed item goes to a fixer (Opus: the bug got past a review), the fix
// gets one more blind review, and only a SHIP is checked again. state: {sha, branch, worktree}; base is set in
// the verify pass, where the work is a range on main and a fix needs its own fix/<id> branch.
async function checkWithFixes(id, task, state, base) {
  let { sha, branch, worktree } = state
  let checkWorktree = null
  let check = null
  for (let round = 1; round <= CHECK_ROUNDS; round++) {
    const where = worktree
      ? `Work in the worktree ${worktree} (cd there); branch "${branch}".`
      : `Check out ${sha} detached in your own worktree, and report that worktree's absolute path as worktree.`
    const c = await safe(`check:${id}#${round}`, checkPrompt(where, sha, task), { phase: 'Check', isolation: worktree ? undefined : 'worktree', model: SONNET, effort: 'high', schema: CHECK })
    if (!worktree && c && c.worktree) checkWorktree = c.worktree
    check = checkResult(c)
    if (check.passed || !c || round === CHECK_ROUNDS) break

    const failed = c.items.filter(i => i.status === 'fail').map(i => `${i.item}${i.note ? ' (checker: ' + i.note + ')' : ''}`)
    const what = `The checker ran the app against the brief's checklist and these items FAILED:\n- ${failed.join('\n- ')}\nChecker notes: ${c.notes || 'none'}`
    const fix = await safe(
      `fix:${id}:check#${round}`,
      worktree
        ? `Work ONLY in the worktree ${worktree} (cd there; use git -C ${worktree}); it has branch "${branch}" checked out. Do not create another branch. ${what}\nFix them, run the project's test command (see 'Project commands' in AGENTS.md), and commit on that branch. Return the new sha (git rev-parse HEAD).` + FIX_RULES + TOOLS
        : `Create a new branch "fix/${id}" from the current main (HEAD). ${what}\nFix them in the code that was added by ${base}..${state.sha}, run the project's test command, commit, and report sha, branch and worktree path.` + FIX_RULES + TOOLS,
      { phase: 'Check', isolation: worktree ? undefined : 'worktree', model: OPUS, effort: 'high', schema: FIX }
    )
    if (!fix || fix.sha === sha) { log(`${id}: the fix for the failed check produced no new commit`); break }
    sha = fix.sha
    branch = fix.branch || branch
    worktree = fix.worktree || worktree

    const review = await reviewAt(
      `review:${id}:check#${round}`,
      base ? `You get the brief and the diff git diff ${base}..${sha} (read it in the worktree ${worktree}, branch ${branch})` : `You get the brief and the diff of branch "${branch}" at commit ${sha} (git diff against its base)`,
      task.brief,
      sha
    )
    const held = notes => ({ sha, branch, worktree, checkWorktree, verdict: review ? review.verdict : 'UNKNOWN', openFindings: review ? review.findings : [], check: { passed: false, skipped: false, items: check.items, notes } })
    if (!review) return held('the review of the fix for the failed check did not finish')
    if (review.verdict !== 'SHIP') { log(`${id}: the fix for the failed check did not pass review`); return held('the fix for the failed check did not pass review') }
  }
  if (!check.passed) log(`${id}: check still failing after ${CHECK_ROUNDS} checks`)
  return { sha, branch, worktree, checkWorktree, verdict: 'SHIP', openFindings: [], check }
}

function manualChecks(rows) {
  return rows.flatMap(r => ((r.check && r.check.items) || []).filter(i => i.status === 'cannot_test').map(i => `${r.id}: ${i.item}${i.note ? ' (' + i.note + ')' : ''}`))
}

// ---------------------------------------------------------------- verify pass
// args.verify = [{id, brief, checklist, base, head}]: review + check heavy work that was merged earlier with defer.
if (input.verify) {
  const items = input.verify
  const results = await pipeline(
    items,

    async it => {
      let sha = it.head
      let branch = null
      let worktree = null
      let review = null
      for (let attempt = 0; attempt <= FIX_ROUNDS; attempt++) {
        review = await reviewAt(
          `review:${it.id}#${attempt + 1}`,
          `You get the brief and the diff git diff ${it.base}..${sha} (read it from the main repository; if a fix branch exists use ${branch || 'none'})`,
          it.brief,
          sha
        )
        if (!review) return { id: it.id, state: 'review-failed', verdict: 'UNKNOWN', openFindings: [] }
        if (review.verdict === 'SHIP' || attempt === FIX_ROUNDS) break
        const fix = await safe(
          `fix:${it.id}#${attempt + 1}`,
          branch
            ? `Work ONLY in the worktree ${worktree} (cd there; use git -C ${worktree}). Fix these findings, run the project's test command (see 'Project commands' in AGENTS.md), and commit on branch ${branch}:\n- ${review.findings.join('\n- ')}` + FIX_RULES + TOOLS
            : `Create a new branch "fix/${it.id}" from the current main (HEAD), fix these findings in the code that was added by ${it.base}..${it.head}, run the project's test command, commit, and report sha, branch and worktree path:\n- ${review.findings.join('\n- ')}` + FIX_RULES + TOOLS,
          { phase: 'Review', isolation: branch ? undefined : 'worktree', model: fixModel(attempt), effort: 'high', schema: FIX }
        )
        if (!fix || fix.sha === sha) { log(`${it.id}: the fix produced no new commit`); break }
        sha = fix.sha
        branch = fix.branch || branch
        worktree = fix.worktree || worktree
      }
      return { id: it.id, state: 'reviewed', verdict: review.verdict, openFindings: review.verdict === 'SHIP' ? [] : review.findings, sha, branch, worktree, task: it }
    },

    async r => {
      if (r.state !== 'reviewed' || r.verdict !== 'SHIP') return { ...r, check: { passed: false, skipped: true, items: [], notes: 'not checked: review did not end in SHIP' } }
      return { ...r, ...(await checkWithFixes(r.id, r.task, { sha: r.sha, branch: r.branch, worktree: r.worktree }, r.task.base)) }
    }
  )

  const rows = results.filter(Boolean)
  const fixed = rows.filter(r => r.branch && r.verdict === 'SHIP' && r.check.passed)
  let shipped = { merged: [], tests_passed: true, notes: 'no fix branches to merge' }
  if (fixed.length) {
    phase('Ship')
    shipped = (await shipWithFixes('ship:verify', fixed.map(r => ({ ...r, summary: `fixes for ${r.id}` })), 'deferred verification fixes')) || { merged: [], tests_passed: false, notes: 'ship agent failed' }
  }

  const allReady =
    rows.length === items.length &&
    rows.every(r => r.verdict === 'SHIP' && r.check.passed) &&
    (!fixed.length || (shipped.tests_passed && shipped.merged.length === fixed.length)) &&
    !incomplete.length
  let finished = null
  if (allReady) {
    phase('Finish')
    finished = await finish('finish:verify', 'deferred verification of earlier batches', shipped.head, rows.map(r => `- ${r.id}: verified${r.branch ? ' (fixes merged from ' + r.branch + ')' : ''}`))
  } else {
    log('Not finished: some items are not SHIP + checked, or main is not green; docs and push skipped')
  }

  // Clean: the deferred builders' worktrees, fix worktrees and check worktrees, only once main is pushed
  const ours = [
    ...items.map(it => ({ branch: it.branch || null, worktree: it.worktree || null })),
    ...rows.map(r => ({ branch: r.branch || null, worktree: r.worktree || null })),
    ...rows.map(r => ({ branch: null, worktree: r.checkWorktree || null })),
  ].filter(x => x.branch || x.worktree)
  let cleaned = null
  if (finished && finished.pushed && ours.length) {
    phase('Clean')
    cleaned = await clean('clean:verify', ours)
  } else if (ours.length) {
    log('Not cleaned: main was not pushed; worktrees and branches are kept (see leftovers)')
  }

  return {
    pushed: !!(finished && finished.pushed),
    finish: finished,
    cleanup: cleaned,
    leftovers: cleaned ? [] : ours,
    shipNotes: shipped.notes,
    items: rows.map(r => ({ id: r.id, verdict: r.verdict, openFindings: r.openFindings, fixBranch: r.branch || null, checkPassed: r.check.passed, checkItems: r.check.items, checkNotes: r.check.notes })),
    manualChecks: manualChecks(rows),
    incomplete,
  }
}

// ---------------------------------------------------------------- normal pass
phase('Plan')
const plan = await safe(
  'plan',
  `Read the project memory (docs/memory/index.md and the notes it links that relate to this request), then the codebase, and plan this request. Do not write code.\n\n` +
  `Split the request into INDEPENDENT tasks that touch different files, so they can be built in parallel (at most ${MAX_TASKS}; prefer 1 task if the request is small). Notes and docs for a change go in the same task as its code, never in a separate task. Builders are Sonnet, so write each brief concretely: exact behaviour, acceptance criteria, files to touch.\n\n` +
  `Mark each task with a weight:\n` +
  `- "light": small, low-risk, few files, clear requirements, nothing in the core state, rendering or data paths. Light tasks are built and tested only; there is no review and no live check.\n` +
  `- "heavy": medium or large; core architecture (state, scheduling, rendering, concurrency, input focus); user data, config or file formats; security; platform APIs or threading; new features with UI; unclear requirements; or anything you are not sure is safe. Heavy tasks get a blind review and a check.\n` +
  `When unsure, pick "heavy".\n\n` +
  `For every heavy task also write "checklist": 3 to 10 concrete things a checker must verify (which demo mode to open, what to compare with which design file, which flows to exercise, which old data to load) and say what must NOT be touched (for example media buttons).\n\n` +
  `Request: ${input.request}` + TOOLS,
  { phase: 'Plan', model: OPUS, effort: 'high', schema: PLAN }
)
if (!plan || !plan.tasks || !plan.tasks.length) throw new Error('Plan failed: ' + incomplete.join('; '))
const tasks = plan.tasks.slice(0, MAX_TASKS)
log(`${tasks.length} task(s): ${tasks.map(t => `${t.id} [${t.weight}]`).join(', ')} (${plan.reason})${input.defer ? '; defer: heavy tasks are verified later' : ''}`)
if (plan.tasks.length > tasks.length) log(`Dropped over-limit tasks: ${plan.tasks.slice(MAX_TASKS).map(t => t.id).join(', ')}`)

const results = await pipeline(
  tasks,

  // Build
  async task => {
    const b = await safe(
      `build:${task.id}`,
      `Build this task. FIRST: your worktree may start from an older commit (the last pushed origin/main), not from the local main that holds all finished work. Before anything else run git log -1 --format=%H main and create your branch from that commit: git checkout -b "task/${task.id}" main. Verify with git merge-base --is-ancestor main HEAD. Then work and commit on that branch. Commit early and often (a small WIP commit every time something compiles or a milestone is done) so the work survives if you are cut off; amend or squash at the end is optional. Test first: for each new behaviour or bug fix, write a test that fails, then the code that makes it pass; if some part has no test setup, say so in your summary. Run the relevant tests (the project's test command, see 'Project commands' in AGENTS.md) before finishing and make sure the build is warning-free. Report the branch, the final commit sha (git rev-parse HEAD) and this worktree's absolute path (git rev-parse --show-toplevel).\n\nBrief:\n${task.brief}` + TOOLS,
      { phase: 'Build', isolation: 'worktree', model: SONNET, effort: 'high', schema: BUILD }
    )
    return b ? { ...b, id: task.id, weight: task.weight, state: 'built' } : { id: task.id, weight: task.weight, state: 'build-failed' }
  },

  // Blind review <-> fix loop, on a pinned commit, heavy tasks only
  async (r, task) => {
    if (r.state !== 'built') return r
    if (task.weight !== 'heavy') return { ...r, verdict: 'SKIPPED', openFindings: [] }
    if (input.defer) return { ...r, verdict: 'DEFERRED', openFindings: [] }
    let sha = r.sha
    let review = null
    for (let attempt = 0; attempt <= FIX_ROUNDS; attempt++) {
      review = await reviewAt(`review:${task.id}#${attempt + 1}`, `You get the brief and the diff of branch "${r.branch}" at commit ${sha} (git diff against its base)`, task.brief, sha)
      if (!review) return { ...r, verdict: 'UNKNOWN', openFindings: [], note: 'review failed' }
      if (review.verdict === 'SHIP' || attempt === FIX_ROUNDS) break
      const fix = await safe(
        `fix:${task.id}#${attempt + 1}`,
        `Work ONLY in the builder's worktree ${r.worktree} (cd there; use git -C ${r.worktree}); it has branch "${r.branch}" checked out. Do not create another branch and do not check the branch out anywhere else. Fix these findings, run the project's test command, and commit on that branch. Return the new sha (git rev-parse HEAD).\n- ${review.findings.join('\n- ')}` + FIX_RULES + TOOLS,
        { phase: 'Review', model: fixModel(attempt), effort: 'high', schema: FIX }
      )
      if (!fix || fix.sha === sha) { log(`${task.id}: the fix produced no new commit`); break }
      sha = fix.sha
    }
    if (review.verdict !== 'SHIP') log(`${task.id}: still FIX after ${FIX_ROUNDS} rounds`)
    return { ...r, sha, verdict: review.verdict, openFindings: review.verdict === 'SHIP' ? [] : review.findings }
  },

  // Check against the written checklist, heavy tasks only, in the builder's worktree; failures loop back once
  async (r, task) => {
    if (r.state !== 'built') return r
    if (r.verdict === 'SKIPPED' || r.verdict === 'DEFERRED') return { ...r, check: { passed: true, skipped: true, items: [], notes: r.verdict === 'SKIPPED' ? 'light task: the builder ran the tests and Ship tests main' : 'deferred to the verify pass' } }
    if (r.verdict !== 'SHIP') return { ...r, check: { passed: false, skipped: true, items: [], notes: 'not checked: review did not end in SHIP' } }
    const out = await checkWithFixes(task.id, task, { sha: r.sha, branch: r.branch, worktree: r.worktree }, null)
    return { ...r, sha: out.sha, verdict: out.verdict, openFindings: out.openFindings, check: out.check }
  }
)

// Ship: barrier on purpose, merging needs every finished task together.
const done = results.filter(Boolean)
const ready = done.filter(r => r.state === 'built' && ['SHIP', 'SKIPPED', 'DEFERRED'].includes(r.verdict) && r.check.passed)
const held = done.filter(r => !ready.includes(r))
if (held.length) log(`Not shipped: ${held.map(r => r.id).join(', ')}`)

let shipped = { merged: [], base: '', head: '', tests_passed: false, notes: 'Nothing passed' }
if (ready.length) {
  phase('Ship')
  shipped = (await shipWithFixes('ship:main', ready, input.request)) || { merged: [], base: '', head: '', tests_passed: false, notes: 'the ship agent failed' }
}

// Finish: docs + push only when nothing is held, deferred or incomplete and main is green
const allReady =
  !input.defer &&
  done.length === tasks.length &&
  ready.length === done.length &&
  shipped.tests_passed &&
  shipped.merged.length === ready.length &&
  !incomplete.length
let finished = null
if (allReady) {
  phase('Finish')
  finished = await finish('finish:main', input.request, shipped.head, ready.map(r => `- ${r.id} [${r.weight}]: ${r.summary}`))
} else {
  log(input.defer ? 'Deferred run: docs and push wait for the verify pass' : 'Not finished: some tasks are held, failed or main is not green; docs and push skipped')
}

// Clean: only after Finish pushed main. Held tasks keep their worktrees (see leftovers)
let cleaned = null
if (finished && finished.pushed) {
  phase('Clean')
  cleaned = await clean('clean:main', ready.map(r => ({ branch: r.branch, worktree: r.worktree })))
}
const leftovers = done
  .filter(r => r.worktree && !(cleaned && ready.includes(r)))
  .map(r => ({ task: r.id, branch: r.branch || null, worktree: r.worktree }))

const byId = Object.fromEntries(tasks.map(t => [t.id, t]))
return {
  pushed: !!(finished && finished.pushed),
  finish: finished,
  cleanup: cleaned,
  // worktrees this run did not hand to Clean; deferred ones are cleaned by the verify pass
  leftovers,
  shipNotes: shipped.notes,
  testsPassedOnMain: shipped.tests_passed,
  base: shipped.base,
  head: shipped.head,
  tasks: done.map(r => ({
    task: r.id,
    weight: r.weight,
    state: r.state,
    branch: r.branch || null,
    sha: r.sha || null,
    worktree: r.worktree || null,
    verdict: r.verdict || null,
    merged: shipped.merged.includes(r.branch),
    summary: r.summary || null,
    openFindings: r.openFindings || [],
    check: r.check || null,
  })),
  // heavy work built with defer: feed this to a later run as args.verify
  deferred: input.defer
    ? done.filter(r => r.verdict === 'DEFERRED' && shipped.merged.includes(r.branch)).map(r => ({ id: r.id, brief: byId[r.id].brief, checklist: byId[r.id].checklist || [], base: shipped.base, head: shipped.head, branch: r.branch, worktree: r.worktree }))
    : [],
  manualChecks: manualChecks(done),
  incomplete,
}
