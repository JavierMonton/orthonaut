type LoadingSpinnerProps = {
  label?: string
}

export default function LoadingSpinner({ label = 'Checking...' }: LoadingSpinnerProps) {
  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/30">
      <div className="rounded-lg bg-white px-6 py-5 shadow-lg">
        <div className="flex items-center gap-3">
          <div className="h-5 w-5 animate-spin rounded-full border-2 border-slate-300 border-t-blue-600" />
          <span className="text-sm text-slate-700">{label}</span>
        </div>
      </div>
    </div>
  )
}
