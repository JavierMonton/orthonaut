import { useState } from 'react'

type AlwaysWrongWordsManagerProps = {
  count: number
  onAdd: (word: string) => Promise<void>
  onExport: () => Promise<void>
}

export default function AlwaysWrongWordsManager({ count, onAdd, onExport }: AlwaysWrongWordsManagerProps) {
  const [input, setInput] = useState('')
  const [adding, setAdding] = useState(false)
  const [exporting, setExporting] = useState(false)

  const handleAdd = async () => {
    const word = input.trim().toLowerCase()
    if (!word) return
    setAdding(true)
    try {
      await onAdd(word)
      setInput('')
    } finally {
      setAdding(false)
    }
  }

  const handleExport = async () => {
    setExporting(true)
    try {
      await onExport()
    } finally {
      setExporting(false)
    }
  }

  return (
    <div className="rounded-xl border border-orange-200 bg-orange-50 p-3 shadow-sm">
      <p className="mb-2 text-sm font-medium text-orange-800">
        Palabras siempre incorrectas{count > 0 ? ` (${count})` : ''}
      </p>
      <div className="flex gap-2">
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') void handleAdd() }}
          placeholder="Añadir palabra..."
          disabled={adding}
          className="flex-1 rounded-md border border-orange-300 bg-white px-3 py-1.5 text-sm outline-none ring-orange-500 transition focus:ring-2 disabled:opacity-60"
        />
        <button
          type="button"
          onClick={() => void handleAdd()}
          disabled={adding || input.trim().length === 0}
          className="rounded-md bg-orange-600 px-3 py-1.5 text-sm font-medium text-white transition hover:bg-orange-700 disabled:cursor-not-allowed disabled:opacity-60"
        >
          Añadir
        </button>
        <button
          type="button"
          onClick={() => void handleExport()}
          disabled={exporting || count === 0}
          className="rounded-md bg-slate-800 px-3 py-1.5 text-sm font-medium text-white transition hover:bg-slate-900 disabled:cursor-not-allowed disabled:opacity-60"
        >
          Exportar a archivo
        </button>
      </div>
    </div>
  )
}
