import {
  MySQL,
  PostgreSQL,
  type SQLNamespace,
  SQLite,
  sql,
} from "@codemirror/lang-sql";
import { Compartment, EditorState, Prec } from "@codemirror/state";
import { oneDark } from "@codemirror/theme-one-dark";
import { EditorView, keymap, placeholder } from "@codemirror/view";
import { basicSetup } from "codemirror";
import { useEffect, useImperativeHandle, useRef } from "react";
import type { Ref } from "react";
import type { DbType } from "../../../api/admin/dbManager";
import { useThemeStore } from "../../../stores/theme";

export type SqlEditorHandle = {
  getValue: () => string;
  setValue: (value: string) => void;
  /** Selected text, or an empty string. */
  getSelection: () => string;
  getCursor: () => number;
  /** Selects a range, e.g. the statement that is about to run. */
  select: (from: number, to: number) => void;
  insert: (text: string) => void;
  focus: () => void;
};

const dialects = { postgres: PostgreSQL, mysql: MySQL, sqlite: SQLite };

function language(dbType: DbType, tables?: Record<string, string[]>) {
  return sql({
    dialect: dialects[dbType],
    schema: tables as SQLNamespace | undefined,
    upperCaseKeywords: true,
  });
}

export function SqlEditor({
  handleRef,
  initialValue = "",
  dbType,
  tables,
  readOnly = false,
  placeholderText,
  onChange,
  onRun,
  onSave,
  className,
}: {
  handleRef?: Ref<SqlEditorHandle>;
  initialValue?: string;
  dbType: DbType;
  /** Table → columns, for completion. */
  tables?: Record<string, string[]>;
  readOnly?: boolean;
  placeholderText?: string;
  onChange?: (value: string) => void;
  /** Ctrl/⌘ + Enter. */
  onRun?: () => void;
  /** Ctrl/⌘ + S. */
  onSave?: () => void;
  className?: string;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const languageSlot = useRef(new Compartment());
  const themeSlot = useRef(new Compartment());
  const readOnlySlot = useRef(new Compartment());
  const callbacks = useRef({ onChange, onRun, onSave });
  callbacks.current = { onChange, onRun, onSave };
  const resolvedTheme = useThemeStore((state) => state.resolved);
  // Only the first value is used; later changes go through the handle.
  const initial = useRef({
    value: initialValue,
    dbType,
    tables,
    readOnly,
    placeholderText,
    dark: resolvedTheme === "dark",
  });

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const start = initial.current;
    const view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: start.value,
        extensions: [
          basicSetup,
          Prec.highest(
            keymap.of([
              {
                key: "Mod-Enter",
                run: () => {
                  callbacks.current.onRun?.();
                  return true;
                },
              },
              {
                key: "Mod-s",
                preventDefault: true,
                run: () => {
                  callbacks.current.onSave?.();
                  return true;
                },
              },
            ]),
          ),
          languageSlot.current.of(language(start.dbType, start.tables)),
          themeSlot.current.of(start.dark ? oneDark : []),
          readOnlySlot.current.of([
            EditorState.readOnly.of(start.readOnly),
            EditorView.editable.of(!start.readOnly),
          ]),
          start.placeholderText ? placeholder(start.placeholderText) : [],
          EditorView.lineWrapping,
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              callbacks.current.onChange?.(update.state.doc.toString());
            }
          }),
        ],
      }),
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: languageSlot.current.reconfigure(language(dbType, tables)),
    });
  }, [dbType, tables]);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: themeSlot.current.reconfigure(
        resolvedTheme === "dark" ? oneDark : [],
      ),
    });
  }, [resolvedTheme]);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: readOnlySlot.current.reconfigure([
        EditorState.readOnly.of(readOnly),
        EditorView.editable.of(!readOnly),
      ]),
    });
  }, [readOnly]);

  useImperativeHandle(
    handleRef,
    () => ({
      getValue: () => viewRef.current?.state.doc.toString() ?? "",
      setValue: (value) => {
        const view = viewRef.current;
        if (!view) return;
        view.dispatch({
          changes: { from: 0, to: view.state.doc.length, insert: value },
        });
      },
      getSelection: () => {
        const view = viewRef.current;
        if (!view) return "";
        const { from, to } = view.state.selection.main;
        return view.state.sliceDoc(from, to);
      },
      getCursor: () => viewRef.current?.state.selection.main.head ?? 0,
      select: (from, to) => {
        viewRef.current?.dispatch({
          selection: { anchor: from, head: to },
          scrollIntoView: true,
        });
      },
      insert: (text) => {
        const view = viewRef.current;
        if (!view) return;
        view.dispatch(view.state.replaceSelection(text));
        view.focus();
      },
      focus: () => viewRef.current?.focus(),
    }),
    [],
  );

  return (
    <div
      ref={hostRef}
      className={`db-sql-editor${className ? ` ${className}` : ""}`}
    />
  );
}
