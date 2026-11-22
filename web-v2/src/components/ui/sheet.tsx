import { Dialog as SheetPrimitive } from 'radix-ui'
import { X } from 'lucide-react'
import type * as React from 'react'
import { cn } from '@/lib/utils'

const Sheet = SheetPrimitive.Root
const SheetTrigger = SheetPrimitive.Trigger
const SheetClose = SheetPrimitive.Close
const SheetTitle = SheetPrimitive.Title
const SheetDescription = SheetPrimitive.Description

/**
 * Side drawer. Enters and exits along the same path (right on desktop,
 * bottom on phones) with the iOS drawer curve; a dimming scrim marks it modal.
 */
function SheetContent({
  className,
  children,
  side = 'right',
  ...props
}: React.ComponentProps<typeof SheetPrimitive.Content> & { side?: 'right' | 'bottom' }) {
  return (
    <SheetPrimitive.Portal>
      <SheetPrimitive.Overlay className="fixed inset-0 z-[60] bg-[var(--scrim)] data-[state=open]:animate-[overlay-in_280ms_ease-out] data-[state=closed]:animate-[overlay-out_200ms_ease-out_forwards]" />
      <SheetPrimitive.Content
        className={cn(
          'fixed z-[61] flex flex-col bg-bg2 shadow-lift outline-none',
          side === 'right' &&
            'inset-y-0 right-0 w-full max-w-[440px] border-l border-line2 data-[state=open]:animate-[sheet-in-right_420ms_var(--ease-drawer)] data-[state=closed]:animate-[sheet-out-right_260ms_var(--ease-drawer)_forwards]',
          side === 'bottom' &&
            'inset-x-0 bottom-0 max-h-[86dvh] rounded-t-2xl border-t border-line2 pb-[env(safe-area-inset-bottom,0px)] data-[state=open]:animate-[sheet-in-bottom_420ms_var(--ease-drawer)] data-[state=closed]:animate-[sheet-out-bottom_260ms_var(--ease-drawer)_forwards]',
          className,
        )}
        {...props}
      >
        {children}
        <SheetPrimitive.Close className="pressable absolute top-3.5 right-3.5 grid size-8 place-items-center rounded-md text-ink3 hover:bg-bg3 hover:text-ink" aria-label="Close">
          <X className="size-4" />
        </SheetPrimitive.Close>
      </SheetPrimitive.Content>
    </SheetPrimitive.Portal>
  )
}
export { Sheet, SheetTrigger, SheetClose, SheetContent, SheetTitle, SheetDescription }
