import { forwardRef, useId } from 'react';
import { classNames } from '@utils/classNames';

export const Input = forwardRef(function Input(
  {
    label,
    error,
    helperText,
    type = 'text',
    disabled = false,
    required = false,
    fullWidth = false,
    className,
    inputClassName,
    ...props
  },
  ref
) {
  const id = useId();
  const inputId = props.id || id;
  const errorId = `${inputId}-error`;
  const helperId = `${inputId}-helper`;

  const hasError = !!error;

  return (
    <div className={classNames('flex flex-col', fullWidth && 'w-full', className)}>
      {label && (
        <label
          htmlFor={inputId}
          className={classNames(
            'mb-1 text-sm font-medium',
            hasError ? 'text-red-600' : 'text-gray-700',
            disabled && 'opacity-50'
          )}
        >
          {label}
          {required && <span className="text-red-500 ml-1">*</span>}
        </label>
      )}

      <input
        ref={ref}
        id={inputId}
        type={type}
        disabled={disabled}
        required={required}
        aria-invalid={hasError}
        aria-describedby={
          hasError ? errorId : helperText ? helperId : undefined
        }
        className={classNames(
          'px-3 py-2 border rounded-md shadow-sm',
          'focus:outline-none focus:ring-2 focus:ring-offset-0',
          'transition-colors duration-200',
          hasError
            ? 'border-red-300 focus:border-red-500 focus:ring-red-500'
            : 'border-gray-300 focus:border-blue-500 focus:ring-blue-500',
          disabled && 'bg-gray-100 cursor-not-allowed opacity-50',
          fullWidth && 'w-full',
          inputClassName
        )}
        {...props}
      />

      {hasError && (
        <p id={errorId} className="mt-1 text-sm text-red-600" role="alert">
          {error}
        </p>
      )}

      {!hasError && helperText && (
        <p id={helperId} className="mt-1 text-sm text-gray-500">
          {helperText}
        </p>
      )}
    </div>
  );
});

export default Input;
