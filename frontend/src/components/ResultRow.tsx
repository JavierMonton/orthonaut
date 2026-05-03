import { Button } from '@headlessui/react'

import type { ArticleResult } from '../types'

type ResultRowProps = {
  result: ArticleResult
  onDelete: (id: number) => Promise<void>
  onMarkValid: (word: string) => Promise<void>
  ignoredWords: Set<string>
}

export default function ResultRow({ result, onDelete, onMarkValid, ignoredWords }: ResultRowProps) {
  const handleDelete = async () => {
    const accepted = window.confirm(`Delete "${result.title}" from database?`)
    if (!accepted) {
      return
    }
    await onDelete(result.id)
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
          <p className="mt-1 text-xs text-slate-500">Revision: {result.revision_id}</p>
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
        <ul className="space-y-1 text-sm text-slate-700">
          {visibleWords.map((word) => (
            <li key={word} className="flex items-center justify-between gap-3 rounded bg-slate-100 px-2 py-1">
              <span>{word}</span>
              <Button
                type="button"
                onClick={() => void onMarkValid(word)}
                className="rounded bg-emerald-600 px-2 py-1 text-xs font-medium text-white transition hover:bg-emerald-700"
              >
                Valid word
              </Button>
            </li>
          ))}
        </ul>
      )}
    </article>
  )
}
