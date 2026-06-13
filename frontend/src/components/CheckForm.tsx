import { Button } from '@headlessui/react'
import type { FormEvent } from 'react'

type CheckFormProps = {
  url: string
  loading: boolean
  onUrlChange: (url: string) => void
  onSubmit: () => Promise<void>
  onAnalyzeRandom: () => Promise<void>
}

export default function CheckForm({ url, loading, onUrlChange, onSubmit, onAnalyzeRandom }: CheckFormProps) {
  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault()
    await onSubmit()
  }

  return (
    <form onSubmit={handleSubmit} className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm">
      <div className="flex flex-col gap-3 sm:flex-row">
        <input
          type="url"
          value={url}
          onChange={(event) => onUrlChange(event.target.value)}
          placeholder="https://es.wikipedia.org/wiki/Wikipedia:Portada"
          className="w-full rounded-md border border-slate-300 px-3 py-2 text-sm outline-none ring-blue-500 transition focus:ring-2"
          disabled={loading}
          required
        />
        <div className="flex gap-2">
          <Button
            type="submit"
            disabled={loading}
            className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white transition hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-60"
          >
            Start
          </Button>
          <Button
            type="button"
            onClick={() => void onAnalyzeRandom()}
            disabled={loading}
            className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white transition hover:bg-indigo-700 disabled:cursor-not-allowed disabled:opacity-60"
          >
            Analyze random page
          </Button>
        </div>
      </div>
    </form>
  )
}
