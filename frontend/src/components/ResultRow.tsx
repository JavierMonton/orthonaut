import { useState } from 'react'

import { Button } from '@headlessui/react'

import { applyEdit, getWordContexts } from '../api'
import type { ApplyEditResponse, ArticleResult, WordContextsResponse } from '../types'

type ResultRowProps = {
  result: ArticleResult
  onDelete: (id: number) => Promise<void>
  onMarkValid?: (word: string) => Promise<void>
  onIgnore?: (word: string) => void
  ignoredWords?: Set<string>
  isLoggedIn: boolean
  oauthConfigured: boolean
  onFetchContexts?: (word: string) => Promise<WordContextsResponse>
  onApplyEdit?: (word: string, replacement: string, occurrenceIndex?: number) => Promise<ApplyEditResponse>
}

type EditState = {
  loading: boolean
  ok?: boolean
  revision?: number
  error?: string
}

function highlightWord(text: string, word: string): React.ReactNode[] {
  const escaped = word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const parts = text.split(new RegExp(`(${escaped})`, 'gi'))
  return parts.map((part, i) =>
    part.toLowerCase() === word.toLowerCase() ? (
      <mark key={i} className="bg-transparent font-bold text-red-600">
        {part}
      </mark>
    ) : (
      <span key={i}>{part}</span>
    ),
  )
}

export default function ResultRow({
  result,
  onDelete,
  onMarkValid,
  onIgnore,
  ignoredWords = new Set(),
  isLoggedIn,
  oauthConfigured,
  onFetchContexts,
  onApplyEdit,
}: ResultRowProps) {
  const [expandedWord, setExpandedWord] = useState<string | null>(null)
  const [contextData, setContextData] = useState<Record<string, string[]>>({})
  const [wikitextData, setWikitextData] = useState<Record<string, string[]>>({})
  const [viewMode, setViewMode] = useState<Record<string, 'plain' | 'wikitext'>>({})
  const [contextIndex, setContextIndex] = useState<Record<string, number>>({})
  const [contextLoading, setContextLoading] = useState<Record<string, boolean>>({})
  const [replacement, setReplacement] = useState<Record<string, string>>({})
  const [editState, setEditState] = useState<Record<string, EditState>>({})

  const handleDelete = async () => {
    const accepted = window.confirm(`Delete "${result.title}"?`)
    if (!accepted) return
    await onDelete(result.id)
  }

  const handleExpand = async (word: string) => {
    if (expandedWord === word) {
      setExpandedWord(null)
      return
    }
    setExpandedWord(word)
    if (contextData[word]) return

    setContextLoading((prev) => ({ ...prev, [word]: true }))
    try {
      const fetchFn = onFetchContexts ?? ((w: string) => getWordContexts(result.id, w))
      const data = await fetchFn(word)
      setContextData((prev) => ({ ...prev, [word]: data.paragraphs }))
      setWikitextData((prev) => ({ ...prev, [word]: data.wikitext_paragraphs }))
      setContextIndex((prev) => ({ ...prev, [word]: 0 }))
      setReplacement((prev) => ({ ...prev, [word]: word }))
    } catch {
      setContextData((prev) => ({ ...prev, [word]: [] }))
    } finally {
      setContextLoading((prev) => ({ ...prev, [word]: false }))
    }
  }

  const handleApplyEdit = async (word: string, occurrenceIndex?: number) => {
    const rep = replacement[word] ?? word
    if (!rep.trim() || rep.trim() === word) return

    setEditState((prev) => ({ ...prev, [word]: { loading: true } }))
    try {
      const editFn = onApplyEdit ?? ((w: string, r: string, idx?: number) => applyEdit(result.id, w, r, idx))
      const res = await editFn(word, rep.trim(), occurrenceIndex)
      setEditState((prev) => ({
        ...prev,
        [word]: { loading: false, ok: true, revision: res.new_revision },
      }))
    } catch (err) {
      setEditState((prev) => ({
        ...prev,
        [word]: {
          loading: false,
          error: err instanceof Error ? err.message : 'Edit failed',
        },
      }))
    }
  }

  const visibleWords = result.wrong_words.filter((word) => !ignoredWords.has(word))

  return (
    <article className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
      <div className="mb-3 flex items-start justify-between gap-3">
        <div>
          <a
            href={result.url}
            target="_blank"
            rel="noreferrer"
            className="text-base font-semibold text-blue-700 hover:underline"
          >
            {result.title}
          </a>
          {result.revision_id && (
            <p className="mt-1 text-xs text-slate-500">Revision: {result.revision_id}</p>
          )}
        </div>
        <Button
          type="button"
          onClick={handleDelete}
          className="rounded-md bg-red-600 px-3 py-1.5 text-xs font-medium text-white transition hover:bg-red-700"
        >
          Delete
        </Button>
      </div>

      {visibleWords.length === 0 ? (
        <p className="text-sm text-slate-500">All words in this result are currently ignored.</p>
      ) : (
        <ul className="space-y-2 text-sm text-slate-700">
          {visibleWords.map((word) => {
            const isExpanded = expandedWord === word
            const paragraphs = contextData[word] ?? []
            const wikitextParagraphs = wikitextData[word] ?? []
            const mode = viewMode[word] ?? 'plain'
            const idx = contextIndex[word] ?? 0
            const isLoadingCtx = contextLoading[word] ?? false
            const edit = editState[word]

            return (
              <li key={word}>
                <div className="flex items-center justify-between gap-3 rounded bg-slate-100 px-2 py-1">
                  <span>{word}</span>
                  <div className="flex gap-1.5">
                    <Button
                      type="button"
                      onClick={() => void handleExpand(word)}
                      className="rounded bg-blue-600 px-2 py-1 text-xs font-medium text-white transition hover:bg-blue-700"
                    >
                      {isExpanded ? 'Collapse' : 'Expand'}
                    </Button>
                    {onMarkValid && (
                      <Button
                        type="button"
                        onClick={() => void onMarkValid(word)}
                        className="rounded bg-emerald-600 px-2 py-1 text-xs font-medium text-white transition hover:bg-emerald-700"
                      >
                        Valid word
                      </Button>
                    )}
                    {onIgnore && (
                      <Button
                        type="button"
                        onClick={() => onIgnore(word)}
                        className="rounded bg-slate-400 px-2 py-1 text-xs font-medium text-white transition hover:bg-slate-500"
                      >
                        Ignore
                      </Button>
                    )}
                  </div>
                </div>

                {isExpanded && (
                  <div className="mt-1 rounded border border-slate-200 bg-slate-50 p-3 text-xs text-slate-700">
                    {isLoadingCtx ? (
                      <p className="text-slate-500">Loading context…</p>
                    ) : paragraphs.length === 0 && wikitextParagraphs.length === 0 ? (
                      <p className="text-slate-500">No paragraph context found for this word.</p>
                    ) : (
                      <>
                        <div className="mb-2 flex items-center gap-3">
                          <p className="text-slate-500">
                            Paragraph {idx + 1} of {mode === 'plain' ? paragraphs.length : wikitextParagraphs.length}
                          </p>
                          <div className="flex rounded border border-slate-300 overflow-hidden text-xs">
                            <button
                              type="button"
                              onClick={() => {
                                setViewMode((prev) => ({ ...prev, [word]: 'plain' }))
                                setContextIndex((prev) => ({ ...prev, [word]: 0 }))
                              }}
                              className={`px-2 py-0.5 transition ${mode === 'plain' ? 'bg-slate-600 text-white' : 'bg-white text-slate-600 hover:bg-slate-100'}`}
                            >
                              Plain
                            </button>
                            <button
                              type="button"
                              onClick={() => {
                                setViewMode((prev) => ({ ...prev, [word]: 'wikitext' }))
                                setContextIndex((prev) => ({ ...prev, [word]: 0 }))
                              }}
                              disabled={wikitextParagraphs.length === 0}
                              className={`px-2 py-0.5 transition ${mode === 'wikitext' ? 'bg-slate-600 text-white' : 'bg-white text-slate-600 hover:bg-slate-100'} disabled:opacity-40 disabled:cursor-not-allowed`}
                            >
                              Wikitext
                            </button>
                          </div>
                        </div>
                        {mode === 'wikitext' ? (
                          <pre className="whitespace-pre-wrap break-words font-mono leading-relaxed">
                            {highlightWord(wikitextParagraphs[idx] ?? '', word)}
                          </pre>
                        ) : (
                          <p className="leading-relaxed">{highlightWord(paragraphs[idx], word)}</p>
                        )}
                        {(mode === 'plain' ? paragraphs : wikitextParagraphs).length > 1 && (
                          <div className="mt-2 flex gap-2">
                            <button
                              type="button"
                              onClick={() =>
                                setContextIndex((prev) => ({ ...prev, [word]: Math.max(0, idx - 1) }))
                              }
                              disabled={idx === 0}
                              className="rounded bg-slate-200 px-2 py-1 text-xs font-medium text-slate-700 transition hover:bg-slate-300 disabled:cursor-not-allowed disabled:opacity-40"
                            >
                              Previous
                            </button>
                            <button
                              type="button"
                              onClick={() => {
                                const len = (mode === 'plain' ? paragraphs : wikitextParagraphs).length
                                setContextIndex((prev) => ({
                                  ...prev,
                                  [word]: Math.min(len - 1, idx + 1),
                                }))
                              }}
                              disabled={idx === (mode === 'plain' ? paragraphs : wikitextParagraphs).length - 1}
                              className="rounded bg-slate-200 px-2 py-1 text-xs font-medium text-slate-700 transition hover:bg-slate-300 disabled:cursor-not-allowed disabled:opacity-40"
                            >
                              Next
                            </button>
                          </div>
                        )}

                        <div className="mt-3 border-t border-slate-200 pt-3">
                          {!oauthConfigured ? (
                            <p className="text-slate-400 italic">
                              Add <code>[oauth]</code> to orthonaut.toml to enable editing.
                            </p>
                          ) : edit?.ok ? (
                            <p className="text-green-700">
                              Edit applied — new revision {edit.revision}. Check the article on
                              Wikipedia.
                            </p>
                          ) : isLoggedIn ? (
                            <>
                              <label className="mb-1 block text-xs font-medium text-slate-600">
                                Replace with:
                              </label>
                              <div className="flex gap-2">
                                <input
                                  type="text"
                                  value={replacement[word] ?? word}
                                  onChange={(e) =>
                                    setReplacement((prev) => ({ ...prev, [word]: e.target.value }))
                                  }
                                  className="flex-1 rounded border border-slate-300 px-2 py-1 text-xs outline-none ring-blue-500 focus:ring-1"
                                />
                                <button
                                  type="button"
                                  onClick={() => void handleApplyEdit(word, contextIndex[word] ?? 0)}
                                  disabled={
                                    edit?.loading ||
                                    !replacement[word]?.trim() ||
                                    replacement[word]?.trim() === word
                                  }
                                  className="rounded bg-amber-600 px-2 py-1 text-xs font-medium text-white transition hover:bg-amber-700 disabled:cursor-not-allowed disabled:opacity-50"
                                >
                                  {edit?.loading ? 'Saving…' : 'Apply edit'}
                                </button>
                                <button
                                  type="button"
                                  onClick={() => void handleApplyEdit(word, undefined)}
                                  disabled={
                                    edit?.loading ||
                                    !replacement[word]?.trim() ||
                                    replacement[word]?.trim() === word
                                  }
                                  className="rounded bg-slate-600 px-2 py-1 text-xs font-medium text-white transition hover:bg-slate-700 disabled:cursor-not-allowed disabled:opacity-50"
                                >
                                  Apply to all
                                </button>
                              </div>
                              {edit?.error && (
                                <p className="mt-1 text-red-600">{edit.error}</p>
                              )}
                            </>
                          ) : (
                            <p className="text-slate-500">
                              Log in to Wikipedia to apply edits.
                            </p>
                          )}
                        </div>
                      </>
                    )}
                  </div>
                )}
              </li>
            )
          })}
        </ul>
      )}
    </article>
  )
}
