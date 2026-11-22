import { Dialog as DialogPrimitive } from 'radix-ui'
import { X } from 'lucide-react'
import type * as React from 'react'
import { cn } from '@/lib/utils'

const Dialog = DialogPrimitive.Root
const DialogTrigger = DialogPrimitive.Trigger
const DialogClose = DialogPrimitive.Close
const DialogTitle = DialogPrimitive.Title
const DialogDescription = DialogPrimitive.Description

function DialogOverlay({ className, ...props }: React.ComponentProps<typeof DialogPrimitive.Overlay>) {
  return (
    <DialogPrimitive.Overlay
      className={cn(
        'fixed inset-0 z-[60] bg-[var(--scrim)] backdrop-blur-[3px] data-[state=open]:animate-[overlay-in_200ms_ease-out] data-[state=closed]:animate-[overlay-out_150ms_ease-out_forwards]',
        className,
      )}
      {...props}
    />
  )
}

function DialogContent({ className, children, showClose = true, ...props }: React.ComponentProps<typeof DialogPrimitive.Content> & { showClose?: boolean }) {
  return (
    <DialogPrimitive.Portal>
      <DialogOverlay />
      <DialogPrimitive.Content
        className={cn(
          'fixed top-[14vh] left-1/2 z-[61] w-[calc(100vw-24px)] max-w-lg -translate-x-1/2 rounded-2xl border border-line2 bg-bg2 shadow-lift outline-none data-[state=open]:animate-[pop-in_180ms_var(--ease-out)] data-[state=closed]:animate-[pop-out_120ms_ease-out_forwards]',
          className,
        )}
        {...props}
      >
        {children}
        {showClose && (
          <DialogPrimitive.Close className="pressable absolute top-3 right-3 grid size-8 place-items-center rounded-md text-ink3 hover:bg-bg3 hover:text-ink" aria-label="Close">
            <X className="size-4" />
          </DialogPrimitive.Close>
        )}
      </DialogPrimitive.Content>
    </DialogPrimitive.Portal>
  )
}
export { Dialog, DialogTrigger, DialogClose, DialogContent, DialogTitle, DialogDescription, DialogOverlay }
