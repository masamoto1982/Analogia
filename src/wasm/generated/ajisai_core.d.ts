/* tslint:disable */
/* eslint-disable */

export class AjisaiInterpreter {
    free(): void;
    [Symbol.dispose](): void;
    clear_stack(): void;
    collect_core_words_info(): any;
    collect_stack(): any;
    /**
     * `[name, hasDependents]` for each User word: whether another word or
     * a pipe still reads it.
     */
    collect_user_words_info(): any;
    collect_word_identities(): any;
    /**
     * Run `code`. Answers a Promise so a core that does real async work can
     * drop in without changing the playground.
     */
    execute(code: string): Promise<any>;
    /**
     * The resource ceilings this interpreter runs under, as JSON.
     */
    host_profile(): string;
    lookup_word_definition(name: string): string | undefined;
    lookup_word_description(name: string): string | undefined;
    constructor();
    reset(): any;
    /**
     * `{ kind, text }` for a name in the dictionary, or `null`.
     */
    resolve_host_lookup(name: string): any;
    restore_stack_snapshot(snapshot_json: string): void;
    /**
     * Restore every word it can; answers `[name, reason]` for the rest.
     */
    restore_user_words(words: any): any;
    set_max_execution_steps(steps: number): void;
    /**
     * The stack and the machine (its tanks, pipes and clock), in a form
     * `restore_stack_snapshot` reads back exactly. The playground carries
     * this string into its worker and back, so the machine lives in it too.
     */
    snapshot_stack(): string;
}

/**
 * Route Rust panics to the browser console. Called once by the TS loader.
 */
export function init_panic_hook(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_ajisaiinterpreter_free: (a: number, b: number) => void;
    readonly ajisaiinterpreter_clear_stack: (a: number) => void;
    readonly ajisaiinterpreter_collect_core_words_info: (a: number) => any;
    readonly ajisaiinterpreter_collect_stack: (a: number) => any;
    readonly ajisaiinterpreter_collect_user_words_info: (a: number) => any;
    readonly ajisaiinterpreter_collect_word_identities: (a: number) => any;
    readonly ajisaiinterpreter_execute: (a: number, b: number, c: number) => any;
    readonly ajisaiinterpreter_host_profile: (a: number) => [number, number];
    readonly ajisaiinterpreter_lookup_word_definition: (a: number, b: number, c: number) => [number, number];
    readonly ajisaiinterpreter_lookup_word_description: (a: number, b: number, c: number) => [number, number];
    readonly ajisaiinterpreter_new: () => number;
    readonly ajisaiinterpreter_reset: (a: number) => any;
    readonly ajisaiinterpreter_resolve_host_lookup: (a: number, b: number, c: number) => any;
    readonly ajisaiinterpreter_restore_stack_snapshot: (a: number, b: number, c: number) => [number, number];
    readonly ajisaiinterpreter_restore_user_words: (a: number, b: any) => [number, number, number];
    readonly ajisaiinterpreter_set_max_execution_steps: (a: number, b: number) => void;
    readonly ajisaiinterpreter_snapshot_stack: (a: number) => [number, number];
    readonly init_panic_hook: () => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
