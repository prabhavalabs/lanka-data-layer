import { Switch as SwitchPrimitive } from 'radix-ui'
import type * as React from 'react'
import { cn } from '@/lib/utils'

function Switch({ className, ...props }: React.ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        'peer inline-flex h-[20px] w-[34px] shrink-0 items-center rounded-full border border-transparent bg-line2 outline-none transition-colors duration-200 data-[state=checked]:bg-ink disabled:opacity-50',
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb className="pointer-events-none block size-4 translate-x-[1px] rounded-full bg-white shadow-[0_1px_3px_rgba(0,0,0,0.35)] data-[state=checked]:bg-bg transition-transform duration-200 ease-[cubic-bezier(0.23,1,0.32,1)] data-[state=checked]:translate-x-[15px]" />
    </SwitchPrimitive.Root>
  )
}
export { Switch }
