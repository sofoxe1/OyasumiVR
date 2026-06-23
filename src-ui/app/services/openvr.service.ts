import { ApplicationRef, Injectable } from '@angular/core';
import { listen } from '@tauri-apps/api/event';
import { DeviceUpdateEvent } from '../models/events';
import { invoke } from '@tauri-apps/api/core';
import { OVRDevice, VRDevicePose } from '../models/ovr-device';
import {
  BehaviorSubject,
  distinctUntilChanged,
  map,
  Observable,
  skip,
  startWith,
} from 'rxjs';
import { orderBy } from 'lodash';
import { AppSettingsService } from './app-settings.service';
import { error, info } from '@tauri-apps/plugin-log';

export type VRStatus = 'INACTIVE' | 'INITIALIZING' | 'INITIALIZED';

@Injectable({
  providedIn: 'root',
})
export class VRService {
  private _status: BehaviorSubject<VRStatus> = new BehaviorSubject<VRStatus>('INACTIVE');
  public status: Observable<VRStatus> = this._status.asObservable();
  private _devices: BehaviorSubject<OVRDevice[]> = new BehaviorSubject<OVRDevice[]>([]);
  public devices: Observable<OVRDevice[]> = this._devices.asObservable();

  private _hmd_pose: BehaviorSubject<VRDevicePose> = new BehaviorSubject<VRDevicePose>({quaternion:[0,0,0,0],position:[0,0,0]});
  public hmd_pose: Observable<VRDevicePose> =
    this._hmd_pose.asObservable();

  constructor(
    private appRef: ApplicationRef,
    private appSettings: AppSettingsService,
  ) {}

  async init() {
    this._status.next(await invoke<VRStatus>('vr_status'));
    this.appSettings.settings
      .pipe(
        map((settings) => settings.openVrInitDelayFix),
        startWith(false),
        distinctUntilChanged(),
        skip(1)
      )
      .subscribe((fixEnabled) => {
        this.applyOpenVrInitDelayFix(fixEnabled);
        if (fixEnabled) info('[VR] Applying VR Initialization delay fix');
        else info('[VR] Removing VR initialization delay fix');
      });
    await Promise.all([
      listen<DeviceUpdateEvent>('OVR_DEVICE_UPDATE', (event) =>
        this.onDeviceUpdate(event.payload.device)
      ),
      listen<VRStatus>('VR_STATUS_UPDATE', (event) => this.onStatusUpdate(event.payload)),
    ]);

  }

  public onDeviceUpdate(device: OVRDevice) {
    device = Object.assign({}, device);
    if (device.isTurningOff === null || device.isTurningOff === undefined)
      device.isTurningOff =
        this._devices.value.find((d) => d.index === device.index)?.isTurningOff ?? false;
    if (!device.canPowerOff) device.isTurningOff = false;
    this._devices.next(
      orderBy(
        [device, ...this._devices.value.filter((d) => d.index !== device.index)],
        ['deviceIndex'],
        ['asc']
      )
    );
    this.appRef.tick();
  }

  public async setAnalogGain(analogGain: number): Promise<void> {
    if (typeof analogGain === 'number' && isFinite(analogGain)) {
      return invoke('openvr_set_analog_gain', { analogGain });
    } else {
      error('[VR] Attempted to set analogGain to invalid value'+ analogGain);
      error('[VR] Attempted to set analogGain to invalid value: ' + analogGain);
    }
  }

  public getAnalogGain(): Promise<number> {
    return invoke<number>('vr_get_analog_gain');
  }

  public setSupersampleScale(supersampleScale: number | null): Promise<void> {
    return invoke('vr_set_supersample_scale', { supersampleScale });
  }

  public getSupersampleScale(): Promise<number | null> {
    return invoke<number | null>('vr_get_supersample_scale');
  }

  public setFadeDistance(fadeDistance: number): Promise<void> {
    return invoke('openvr_set_fade_distance', { fadeDistance });
  }

  public getFadeDistance(): Promise<number> {
    return invoke<number>('openvr_get_fade_distance');
  }

  private onStatusUpdate(status: VRStatus) {
    this._status.next(status);
    switch (status) {
      case 'INACTIVE':
      case 'INITIALIZING':
        this._devices.next([]);
        break;
      case 'INITIALIZED':
        break;
    }
  }

  private async getDevices(): Promise<Array<OVRDevice>> {
    // Get devices
    let devices = await invoke<OVRDevice[]>('vr_get_devices');
    // Carry over current local state
    devices = devices.map((device) => {
      device.isTurningOff =
        this._devices.value.find((d) => d.index === device.index)?.isTurningOff ?? false;
      return device;
    });
    // Return newly fetched devices
    return devices;
  }

  private async applyOpenVrInitDelayFix(enabled: boolean) {
    await invoke('openvr_set_init_delay_fix', { enabled });
  }

 
}
