// SPDX-License-Identifier: GPL-3.0-or-later
// Ce que `commandes.ts` importe de `@tauri-apps/api/core`, sans installer le
// paquet : seule la signature d'`invoke` sert au contrôle des types.
export declare function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
