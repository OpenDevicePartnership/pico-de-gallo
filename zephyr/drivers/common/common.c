#include <errno.h>
#include "common.h"
#include "gallo_registry.h"

void *pdg_common_bottom_open(const char *serial)
{
    return (void *)pdg_registry_open(serial);
}

void pdg_common_bottom_close(void *ctx)
{
    pdg_registry_close((const struct PicoDeGallo *)ctx);
}

int pdg_common_status_to_errno(Status status)
{
    // big big list
	switch (status) {
        case Ok:                      return 0;
        case I2cAddressOutOfRange:    return -EINVAL;
        case GpioInvalidPin:          return -EINVAL;
        case UartInvalidBaudRate:     return -EINVAL;
        case PwmInvalidChannel:       return -EINVAL;
        case PwmInvalidDutyCycle:     return -EINVAL;
        case PwmInvalidConfiguration: return -EINVAL;
        case InvalidArgument:         return -EINVAL;
        case BufferTooLong:           return -EMSGSIZE;
        case SchemaMismatch:          return -EMSGSIZE;
        case InvalidResponse:         return -EPROTO;
        case CommsFailed:             return -ECOMM;
        case OneWireNoPresence:       return -ECOMM;
        case Uninitialized:           return -ENODEV;
        case I2cNack:                 return -ENXIO;
        case I2cArbitrationLoss:      return -EAGAIN;
        case GpioWrongDirection:      return -EACCES;
        case GpioPinMonitored:        return -EBUSY;
        case GpioPinNotMonitored:     return -ENOENT;
        case GpioTimeout:             return -ETIMEDOUT;
        case LegacyFirmware:          return -ENOSYS;
        case Unsupported:             return -ENOTSUP;
        case I2cReadFailed:           return -EIO;
        case I2cWriteFailed:          return -EIO;
        case I2cWriteReadFailed:      return -EIO;
        case I2cBusError:             return -EIO;
        case I2cOverrun:              return -EIO;
        case I2cScanFailed:           return -EIO;
        case I2cSetConfigFailed:      return -EIO;
        case I2cGetConfigFailed:      return -EIO;
        case I2cBatchFailed:          return -EIO;
        case PingFailed:              return -EIO;
        case VersionFailed:           return -EIO;
        case DeviceInfoFailed:        return -EIO;
        case SpiReadFailed:           return -EIO;
        case SpiWriteFailed:          return -EIO;
        case SpiFlushFailed:          return -EIO;
        case SpiTransferFailed:       return -EIO;
        case SpiSetConfigFailed:      return -EIO;
        case SpiGetConfigFailed:      return -EIO;
        case SpiBatchFailed:          return -EIO;
        case GpioGetFailed:           return -EIO;
        case GpioPutFailed:           return -EIO;
        case GpioWaitFailed:          return -EIO;
        case GpioSetConfigFailed:     return -EIO;
        case GpioSubscribeFailed:     return -EIO;
        case GpioUnsubscribeFailed:   return -EIO;
        case SetConfigFailed:         return -EIO;
        case UartReadFailed:          return -EIO;
        case UartWriteFailed:         return -EIO;
        case UartFlushFailed:         return -EIO;
        case UartOverrun:             return -EIO;
        case UartBreak:               return -EIO;
        case UartParity:              return -EIO;
        case UartFraming:             return -EIO;
        case UartSetConfigFailed:     return -EIO;
        case UartGetConfigFailed:     return -EIO;
        case PwmSetDutyCycleFailed:   return -EIO;
        case PwmGetDutyCycleFailed:   return -EIO;
        case PwmEnableFailed:         return -EIO;
        case PwmDisableFailed:        return -EIO;
        case PwmSetConfigFailed:      return -EIO;
        case PwmGetConfigFailed:      return -EIO;
        case AdcReadFailed:           return -EIO;
        case AdcGetConfigFailed:      return -EIO;
        case AdcConversionFailed:     return -EIO;
        case OneWireBusError:         return -EIO;
        case OneWireReadFailed:       return -EIO;
        case OneWireWriteFailed:      return -EIO;
        case OneWireSearchFailed:     return -EIO;
        case SystemResetSubscriptionsFailed: return -EIO;
        default:                      return -EIO;
	}
}