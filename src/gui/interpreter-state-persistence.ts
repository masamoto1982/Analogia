import type { AjisaiInterpreter, UserWord } from '../wasm-interpreter-types';
import type { InterpreterStateSnapshot } from '../platform';
import { getPlatform } from '../platform';
import { collectUserWords, toError } from './interpreter-execution-utils';

// These three words exist for one demonstration: a Word button's colour shows
// that another Word calls it, and that is only visible once one seeded word
// calls others. SPEND-RATE over the two rates is the whole of it, so nothing
// else is seeded — a fresh dictionary is for the reader to fill.
export const EXAMPLE_USER_WORDS: UserWord[] = [
    // The rates of a MONIAC-style economy (examples/moniac.alg). SPEND-RATE is
    // what households keep circulating once tax and saving are taken, so
    // while it exists neither rate can be deleted (definitionConflict).
    {
        name: 'TAX-RATE',
        definition: '0.25',
    },
    {
        name: 'SAVING-RATE',
        definition: '0.15',
    },
    {
        name: 'SPEND-RATE',
        definition: '1 TAX-RATE SUB SAVING-RATE SUB',
    },
];

type Result<T, E = Error> =
    | { ok: true; value: T }
    | { ok: false; error: E };

// The persisted session document. `stateVersion` identifies the format; a
// document that does not carry the current version is not migrated — the beta
// reads one format only (LANG.OBSERVATION.FIREWALL).
//
// A bump discards every existing session, so it gates readability, not
// changes: bump it only when an older document cannot be parsed (a field
// inside the stack snapshot changed shape), never to record that a key went
// away — an abandoned key beside the ones a reader looks at is simply never
// read.
const STATE_FORMAT_VERSION = 5;

// Whether a saved session carries a dictionary of its own — including an
// empty one. An empty dictionary is a choice the user made by deleting every
// User Word, and restoring it as empty is what keeps that choice: reseeding
// the Example Words whenever the saved list was empty brought back, on every
// reload, the words the user had just deleted. The Example Words seed a
// session that has no saved dictionary at all — a first visit, or a document
// of another format — and Reset, which asks for them.
export const checkHasSavedDictionary = (state: Pick<InterpreterStateSnapshot, 'userWords'>): boolean =>
    Array.isArray(state.userWords);

interface RestoredSelection {
    readonly activeDictionarySheet?: string;
}

export interface PersistenceCallbacks {
    readonly showError?: (error: Error) => void;
    readonly updateDisplays?: () => void;
    readonly showInfo?: (text: string, append: boolean) => void;
    /** The dictionary sheet currently selected, saved with the session. */
    readonly readActiveDictionarySheet?: () => string;
    // Takes an import in turn with runs, so a run in flight cannot undo it.
    readonly runExclusive?: <T>(task: () => Promise<T>) => Promise<T>;
}

export interface Persistence {
    readonly init: () => Promise<void>;
    readonly saveCurrentState: () => Promise<void>;
    readonly loadDatabaseData: () => Promise<RestoredSelection>;
    readonly fullReset: () => Promise<void>;
    readonly exportUserWords: () => void;
    readonly importUserWords: () => void;
}

const collectCurrentState = (
    interpreter: AjisaiInterpreter,
    activeDictionarySheet: string | undefined
): InterpreterStateSnapshot => ({
    stateVersion: STATE_FORMAT_VERSION,
    // The lossless snapshot is what restore reads. The observation-format
    // stack used to be saved beside it "for display", and nothing ever read
    // it back: a 200,000-element stack was serialized twice on every save.
    stackSnapshot: interpreter.snapshot_stack(),
    userWords: collectUserWords(interpreter),
    activeDictionarySheet
});

// Identity-keyed export/import (LANG.AUTHORITY.FREEDOM). The export document
// carries each word's content identity so a shared group is content-addressed:
// re-importing it is recognised as a no-op (deduplicated), and a definition
// edited without re-exporting is detected via an identity mismatch.
const EXPORT_FORMAT_VERSION = 3;

interface ExportWord {
    readonly name: string;
    readonly definition: string | null;
    readonly description?: string | null;
    readonly id?: string;
}

interface ExportDocument {
    readonly formatVersion: number;
    readonly words: ExportWord[];
}

// Keyed by name, as `collect_word_identities` reports it.
const collectWordIdentityMap = (interpreter: AjisaiInterpreter): Map<string, string> => {
    const map = new Map<string, string>();
    for (const [name, id] of interpreter.collect_word_identities()) {
        map.set(buildWordKey(name), id);
    }
    return map;
};

// Every User Word, unconditionally: User is the only exportable tier, so there
// is nothing to filter by.
export const createExportData = (interpreter: AjisaiInterpreter): ExportDocument => {
    const identities = collectWordIdentityMap(interpreter);
    const words: ExportWord[] = collectUserWords(interpreter).map((word) => {
        const id = identities.get(buildWordKey(word.name));
        return id ? { ...word, id } : word;
    });
    return { formatVersion: EXPORT_FORMAT_VERSION, words };
};

export interface ParsedImport {
    readonly words: UserWord[];
    // Embedded content identities keyed by upper-cased word name, or null when
    // the document carries none.
    readonly embeddedIds: Map<string, string> | null;
}

const DEFAULT_EXPORT_NAME = 'user-words';
// The whole key of a User-tier word: its normalized name.
const buildWordKey = (name: string): string => name.toUpperCase();

// `NAME (why)` for each entry, in the order the interpreter gave them.
const describeSkipped = (skipped: readonly SkippedWord[]): string =>
    skipped.map(({ name, reason }) => `${name} (${reason})`).join('; ');

/** One saved entry the interpreter could not restore, and its reason. */
export interface SkippedWord {
    readonly name: string;
    readonly reason: string;
}

export interface ImportSummary {
    /** Words the dictionary holds now and did not hold with this content before. */
    readonly added: string[];
    /** Words already present with identical content (deduplicated by content identity). */
    readonly unchanged: string[];
    /** Entries the interpreter refused, with its reason for each. */
    readonly skipped: SkippedWord[];
    /** Words whose embedded content identity is not the identity they have here. */
    readonly idMismatches: string[];
}

const toSkippedWords = (
    skipped: ReadonlyArray<readonly [name: string, reason: string]>
): SkippedWord[] => skipped.map(([name, reason]) => ({ name, reason }));

// What an import did, told from what the interpreter answered rather than
// guessed from the dictionary afterwards.
//
// A restore skips an entry it cannot take and names it; comparing identities
// before and after tells an added word from one already there. The old
// reading compared only the dictionaries and so mis-told two cases: a
// refused redefinition of a word another word still calls left the old body
// in place, which read as "unchanged (deduplicated by content identity)" —
// the file's body was different, and it was refused; and an entry with no
// body, which the interpreter passes over, counted as imported.
export const summarizeImport = (
    requested: readonly UserWord[],
    skipped: readonly SkippedWord[],
    before: ReadonlyMap<string, string>,
    after: ReadonlyMap<string, string>,
    embeddedIds: ReadonlyMap<string, string> | null
): ImportSummary => {
    const skippedKeys = new Set(skipped.map(entry => buildWordKey(entry.name)));
    const seen = new Set<string>();
    const added: string[] = [];
    const unchanged: string[] = [];
    const idMismatches: string[] = [];
    for (const word of requested) {
        const key = buildWordKey(word.name);
        if (seen.has(key)) continue;
        seen.add(key);
        if (skippedKeys.has(key)) continue;
        // Passed over without a report: an entry with no body asked for
        // nothing, so its absence is not an arrival to count.
        if (!after.has(key)) continue;
        if (before.has(key) && before.get(key) === after.get(key)) {
            unchanged.push(word.name);
        } else {
            added.push(word.name);
        }
        const expected = embeddedIds?.get(key);
        const actual = after.get(key);
        if (expected && actual && expected !== actual) {
            idMismatches.push(word.name);
        }
    }
    return { added, unchanged, skipped: [...skipped], idMismatches };
};

// Validate a single raw word entry from an untrusted source — an import file,
// or the saved session, which a hand edit or an older build can have left
// malformed. Returns a normalized word, or null when the entry is malformed.
// A word is only usable downstream if it has a string `name`; `definition`
// and `id` are optional and must be strings when present. This keeps
// `parseImportDocument` a total function: a hostile or hand-corrupted file
// with `null`, numeric, or name-less entries is parsed or cleanly rejected,
// never thrown out of the `.map` or out of `buildWordKey`. The saved session
// goes through it for the same reason: `restore_user_words` throws on a list
// it cannot deserialize, and one malformed entry would otherwise cost the
// whole dictionary.
export const normalizeWordEntry = (
    raw: unknown
): { name: string; definition: string | null; description?: string | null; id?: string } | null => {
    if (!raw || typeof raw !== 'object') return null;
    const word = raw as Record<string, unknown>;
    if (typeof word.name !== 'string') return null;
    const definition = typeof word.definition === 'string' ? word.definition : null;
    const description = typeof word.description === 'string' ? word.description : undefined;
    const id = typeof word.id === 'string' ? word.id : undefined;
    return id ? { name: word.name, definition, description, id } : { name: word.name, definition, description };
};

export const parseImportDocument = (jsonString: string): Result<ParsedImport, Error> => {
    let parsed: unknown;
    try {
        parsed = JSON.parse(jsonString);
    } catch (e) {
        return { ok: false, error: toError(e) };
    }

    // Malformed entries are dropped rather than thrown on or forwarded to
    // `restore_user_words`; valid words in a partially-corrupt file still
    // import.
    const collect = (rawWords: unknown[]): ParsedImport => {
        const embeddedIds = new Map<string, string>();
        const words: UserWord[] = [];
        for (const raw of rawWords) {
            const word = normalizeWordEntry(raw);
            if (!word) continue;
            if (word.id) embeddedIds.set(buildWordKey(word.name), word.id);
            words.push({ name: word.name, definition: word.definition, description: word.description });
        }
        return { words, embeddedIds: embeddedIds.size > 0 ? embeddedIds : null };
    };

    // A versioned export document carrying per-word content identities is the
    // only accepted form; the bare array of the alpha format is not read.
    if (parsed && typeof parsed === 'object' && Array.isArray((parsed as ExportDocument).words)) {
        return { ok: true, value: collect((parsed as ExportDocument).words as unknown[]) };
    }

    return {
        ok: false,
        error: new Error('Invalid file format. Expected a versioned export document with a `words` array.')
    };
};

export const createPersistence = (
    interpreter: AjisaiInterpreter,
    callbacks: PersistenceCallbacks = {}
): Persistence => {
    const { showError, updateDisplays, showInfo, readActiveDictionarySheet } = callbacks;
    const runExclusive = callbacks.runExclusive ?? (<T>(task: () => Promise<T>) => task());
    let dbInitialized = false;
    const MAX_RETRY_COUNT = 3;
    const RETRY_DELAY_MS = 1000;

    const sleep = (ms: number): Promise<void> =>
        new Promise(resolve => setTimeout(resolve, ms));

    const init = async (): Promise<void> => {
        for (let attempt = 1; attempt <= MAX_RETRY_COUNT; attempt++) {
            try {
                await getPlatform().persistence.open();
                dbInitialized = true;
                return;
            } catch (error) {
                console.error(`Failed to initialize persistence database (attempt ${attempt}/${MAX_RETRY_COUNT}):`, error);
                if (attempt < MAX_RETRY_COUNT) {
                    await sleep(RETRY_DELAY_MS * attempt);
                }
            }
        }

        console.warn('Persistence database initialization failed after all retries. Data will not be persisted.');
        showError?.(new Error('Failed to initialize database. Changes will not be saved.'));
    };

    const SAVE_DEBOUNCE_MS = 400;
    let saveTimer: ReturnType<typeof setTimeout> | null = null;
    let pendingSave: Promise<void> | null = null;
    let resolvePendingSave: (() => void) | null = null;

    // A failing auto-save is reported once, not on every keystroke's worth of
    // saves after it.
    let saveFailureReported = false;

    const performSave = async (): Promise<void> => {
        if (!dbInitialized) return;

        try {
            const state = collectCurrentState(interpreter, readActiveDictionarySheet?.());
            await getPlatform().persistence.saveInterpreterState(state);
        } catch (error) {
            console.error('Failed to auto-save state:', error);
            if (!saveFailureReported) {
                saveFailureReported = true;
                showError?.(new Error('Failed to save the session; changes since the last save will not survive a reload.'));
            }
        }
    };

    // Auto-save fires after every execution, word edit and dictionary switch.
    // Debouncing coalesces bursts of those operations into a single snapshot
    // + IndexedDB write. The snapshot is taken when the timer fires, so the
    // most recent interpreter state is always the one persisted.
    const flushPendingSave = (): void => {
        if (!saveTimer) return;
        clearTimeout(saveTimer);
        saveTimer = null;
        const resolve = resolvePendingSave;
        pendingSave = null;
        resolvePendingSave = null;
        void performSave().finally(() => resolve?.());
    };

    const saveCurrentState = (): Promise<void> => {
        if (!pendingSave) {
            pendingSave = new Promise<void>(resolve => { resolvePendingSave = resolve; });
        }
        if (saveTimer) clearTimeout(saveTimer);
        saveTimer = setTimeout(flushPendingSave, SAVE_DEBOUNCE_MS);
        return pendingSave;
    };

    // A debounced save would otherwise be lost if the tab is hidden or closed
    // within the debounce window, so flush it immediately on those events.
    if (typeof document !== 'undefined') {
        document.addEventListener('visibilitychange', () => {
            if (document.visibilityState === 'hidden') flushPendingSave();
        });
        window.addEventListener('pagehide', flushPendingSave);
    }

    const loadExampleWords = async (): Promise<void> => {
        try {
            interpreter.restore_user_words(EXAMPLE_USER_WORDS);
            await saveCurrentState();

            const wordNames = EXAMPLE_USER_WORDS.map(w => w.name).join(', ');
            showInfo?.(`Example Words loaded: ${wordNames}`, false);
        } catch (error) {
            console.error('Failed to load Example Words:', error);
            showError?.(toError(error));
        }
    };

    const loadDatabaseData = async (): Promise<RestoredSelection> => {
        if (!dbInitialized) {
            console.warn('Database not initialized, loading Example Words instead.');
            await loadExampleWords();
            return {};
        }

        try {
            const state = await getPlatform().persistence.loadInterpreterState();

            if (state) {
                // One accepted format: a document of the current version whose
                // stack is the lossless snapshot. Anything else — an alpha
                // document, a hand-edited one — starts a fresh session with the
                // Example Words rather than being migrated (LANG.OBSERVATION.FIREWALL).
                if (state.stateVersion !== STATE_FORMAT_VERSION) {
                    console.warn(
                        `Saved state is format ${String(state.stateVersion)}, not ${STATE_FORMAT_VERSION}; loading Example Words.`
                    );
                    await loadExampleWords();
                    return {};
                }
                // The dictionary first, and on its own: a saved stack that
                // does not decode is a lost stack, not a lost dictionary. The
                // two used to be restored stack-first inside one `try`, so a
                // bad snapshot — or one malformed word entry, which makes
                // `restore_user_words` throw — skipped the dictionary, and the
                // next auto-save wrote the empty session over the saved one.
                if (!checkHasSavedDictionary(state)) {
                    await loadExampleWords();
                } else if (state.userWords.length > 0) {
                    const wordsToRestore = (state.userWords as unknown[])
                        .map(normalizeWordEntry)
                        .filter((word): word is NonNullable<typeof word> => word !== null)
                        .map(({ name, definition, description }) => ({ name, definition, description }));
                    const skipped = toSkippedWords(interpreter.restore_user_words(wordsToRestore));
                    if (skipped.length > 0) {
                        showError?.(new Error(
                            `${skipped.length} saved word(s) could not be restored and were left out: ${describeSkipped(skipped)}. The rest of the dictionary was restored.`
                        ));
                    }
                }

                if (typeof state.stackSnapshot === 'string') {
                    try {
                        interpreter.restore_stack_snapshot(state.stackSnapshot);
                    } catch (error) {
                        console.error('Failed to restore the saved stack:', error);
                        showError?.(new Error('The saved stack could not be restored and starts empty; the dictionary was restored.'));
                    }
                }

                return {
                    activeDictionarySheet: state.activeDictionarySheet
                };
            } else {
                await loadExampleWords();
                return {};
            }
        } catch (error) {
            console.error('Failed to load database data:', error);
            showError?.(toError(error));
            return {};
        }
    };

    const exportUserWords = (): void => {
        const requestedName = window.prompt('Export file name', DEFAULT_EXPORT_NAME)?.trim();
        if (!requestedName) {
            return;
        }
        const exportData = createExportData(interpreter);
        const filename = `${requestedName}.json`;

        // Reported under the name the file was saved as: a native save dialog
        // lets the user pick another one.
        getPlatform().fileIO.saveJson(filename, exportData)
            .then((saved) => showInfo?.(`User words exported as ${saved.filename}`, true))
            .catch((error) => showError?.(toError(error)));
    };

    const importUserWords = (): void => {
        // The file is chosen at once — a file dialog needs the click that asked
        // for it — and merged in its turn.
        getPlatform().fileIO.openJsonFile().then((openedFile) => runExclusive(async () => {
            if (!openedFile) {
                return;
            }

            try {
                const parseResult = parseImportDocument(openedFile.text);

                if (!parseResult.ok) {
                    showError?.(parseResult.error);
                    return;
                }

                const { words: importedWords, embeddedIds } = parseResult.value;

                // Content-addressed dedup (LANG.AUTHORITY.FREEDOM): compare identities before and
                // after the merge. Words whose identity is unchanged were already
                // present with identical content and count as deduplicated.
                const before = collectWordIdentityMap(interpreter);
                const skipped = toSkippedWords(interpreter.restore_user_words(importedWords));
                const after = collectWordIdentityMap(interpreter);
                const { added, unchanged, idMismatches } = summarizeImport(
                    importedWords,
                    skipped,
                    before,
                    after,
                    embeddedIds
                );

                updateDisplays?.();
                await saveCurrentState();

                const summary = unchanged.length > 0
                    ? `${added.length} user words imported, ${unchanged.length} unchanged (deduplicated by content identity)`
                    : `${added.length} user words imported and saved`;
                showInfo?.(summary, true);
                // Appended under the count rather than shown as an error: an
                // error is written in place of the output, and the count
                // belongs beside what was left out.
                if (skipped.length > 0) {
                    showInfo?.(
                        `${skipped.length} word(s) in the file were not imported: ${describeSkipped(skipped)}.`,
                        true
                    );
                }
                if (idMismatches.length > 0) {
                    showInfo?.(
                        // Claims no behavioural difference: spec/identity.json
                        // reads a mismatch as `unknown`, never as `different`.
                        // Names both causes — identity is transitive over
                        // dependencies, so an untouched body can still mismatch.
                        `Content identity differs from the exported id (this word's body, or one of its dependencies, is not identical here): ${idMismatches.join(', ')}`,
                        true
                    );
                }

            } catch (error) {
                showError?.(toError(error));
            }
        })).catch((error) => showError?.(toError(error)));
    };

    const fullReset = async (): Promise<void> => {
        try {
            if (dbInitialized) {
                await getPlatform().persistence.clearAll();
            } else {
                console.warn('Database not initialized, skipping clear operation.');
            }
            await loadExampleWords();
            updateDisplays?.();
        } catch (error) {
            console.error('Failed to perform full reset:', error);
            showError?.(toError(error));
        }
    };

    return {
        init,
        saveCurrentState,
        loadDatabaseData,
        fullReset,
        exportUserWords,
        importUserWords
    };
};
