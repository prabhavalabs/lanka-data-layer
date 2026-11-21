import { Tooltip as TooltipPrimitive } from 'radix-ui'
import type * as React from 'react'
import { cn } from '@/lib/utils'

const TooltipProvider = ({ delayDuration = 350, skipDelayDuration = 400, ...props }: React.ComponentProps<typeof TooltipPrimitive.Provider>) => (
  <TooltipPrimitive.Provider delayDuration={delayDuration} skipDelayDuration={skipDelayDuration} {...props} />
)
const Tooltip = TooltipPrimitive.Root
const TooltipTrigger = TooltipPrimitive.Trigger

function TooltipContent({ className, sideOffset = 6, children, ...props }: React.ComponentProps<typeof TooltipPrimitive.Content>) {
  return (
    <TooltipPrimitive.Portal>
      <TooltipPrimitive.Content
        sideOffset={sideOffset}
        className={cn(
          'z-[80] origin-(--radix-tooltip-content-transform-origin) rounded-md border border-line2 bg-bg4 px-2 py-1 text-xs text-ink shadow-soft data-[state=delayed-open]:animate-[pop-in_140ms_var(--ease-out)] data-[state=closed]:animate-[pop-out_100ms_ease-out]',
          className,
        )}
        {...props}
      >
        {children}
      </TooltipPrimitive.Content>
    </TooltipPrimitive.Portal>
  )
}
export { Tooltip, TooltipTrigger, TooltipContent, TooltipProvider }
