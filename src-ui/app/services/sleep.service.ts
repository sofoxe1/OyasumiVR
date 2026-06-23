import { Injectable } from '@angular/core';
import {
  BehaviorSubject,
  filter,
  firstValueFrom,
  map,
  Observable,
  Subject,
} from 'rxjs';
import { SleepModeStatusChangeReason, SleepState } from '../models/sleep-mode';
import { SETTINGS_KEY_SLEEP_MODE, SETTINGS_STORE } from '../globals';
import { SleepingPose } from '../models/sleeping-pose';
import { VRDevicePose } from '../models/ovr-device';
import { info } from '@tauri-apps/plugin-log';
import { NotificationService } from './notification.service';
import { TranslateService } from '@ngx-translate/core';
import { EventLogService } from './event-log.service';
import { EventLogSleepModeDisabled, EventLogSleepModeEnabled } from '../models/event-log-entry';
import { AppSettingsService } from './app-settings.service';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

@Injectable({
  providedIn: 'root',
})
export class SleepService {
  private _mode: BehaviorSubject<boolean | null> = new BehaviorSubject<boolean | null>(null);
  public mode: Observable<boolean> = this._mode.asObservable().pipe(
    filter((v) => v !== null),
    map((v) => v as boolean)
  );
  private forcePose$: Subject<SleepingPose> = new Subject<SleepingPose>();

  private readonly _onSleepModeChange = new Subject<{
    mode: boolean;
    reason: SleepModeStatusChangeReason;
  }>();
  public readonly onSleepModeChange: Observable<{
    mode: boolean;
    reason: SleepModeStatusChangeReason;
  }> = this._onSleepModeChange.asObservable();
  private _hmd_pose: BehaviorSubject<VRDevicePose> = new BehaviorSubject<VRDevicePose>({
    quaternion: [0, 0, 0, 0],
    position: [0, 0, 0],
  });
  private _pose: BehaviorSubject<SleepingPose> = new BehaviorSubject<SleepingPose>('UNKNOWN');

  public pose: Observable<SleepingPose> = this._pose.asObservable();

  constructor(
    // private openvr: VRService,
    private notifications: NotificationService,
    private eventLog: EventLogService,
    private appSettings: AppSettingsService,
    private translate: TranslateService
  ) {}

  async init() {
    this.pose.subscribe((v)=>info("headset pose:"+v));
    // Load default settings
    const settings = await firstValueFrom(this.appSettings.settings);
    let mode: boolean;
    switch (settings.sleepModeStartupBehaviour) {
      case 'PERSIST':
        mode = (await SETTINGS_STORE.get<boolean>(SETTINGS_KEY_SLEEP_MODE)) || false;
        break;
      case 'ACTIVE':
        mode = true;
        break;
      case 'INACTIVE':
        mode = false;
        break;
    }
    this._mode.next(mode);
    // Handle events
    Promise.all([
      await listen<boolean>('setSleepMode', (e) => {
        if (e.payload) {
          this.enableSleepMode({ type: 'MANUAL' });
        } else {
          this.disableSleepMode({ type: 'MANUAL' });
        }
      }),
      await listen<number>('POSE',(v)=>{
        var side="";
        switch (v.payload){
          case 0:
            side="SIDE_BACK";
            break
          case 1:
            side="SIDE_LEFT";
            break
          case 2:
            side="SIDE_RIGHT";
            break
          case 3:
            side="SIDE_FRONT";
            break
        }
        this._pose.next(side as SleepingPose);

      })
    ]);
  }

  forcePose(pose: SleepingPose) {
    this.forcePose$.next(pose);
  }


  async enableSleepMode(reason: SleepModeStatusChangeReason) {
    if (this._mode.value) return;
    await invoke('set_sleep_state', { state: SleepState.Sleeping });
    await invoke('vr_sleep_mode_check', { value: false });
    reason.enabled = true;
    info(`[Sleep] Sleep mode enabled (reason=${reason.type})`);
    this.eventLog.logEvent({
      type: 'sleepModeEnabled',
      reason: reason,
    } as EventLogSleepModeEnabled);
    this._mode.next(true);
    this._onSleepModeChange.next({ mode: true, reason });
    await SETTINGS_STORE.set(SETTINGS_KEY_SLEEP_MODE, true);
    if (await this.notifications.notificationTypeEnabled('SLEEP_MODE_ENABLED')) {
      await this.notifications.send(
        this.translate.instant('notifications.sleepModeEnabled.content')
      );
    }
  }

  async disableSleepMode(reason: SleepModeStatusChangeReason) {
    if (!this._mode.value) return;
    invoke('set_sleep_state', { state: SleepState.Awake });
    reason.enabled = false;
    info(`[Sleep] Sleep mode disabled (reason=${reason.type})`);
    this.eventLog.logEvent({
      type: 'sleepModeDisabled',
      reason,
    } as EventLogSleepModeDisabled);
    this._mode.next(false);
    this._onSleepModeChange.next({ mode: false, reason });
    await SETTINGS_STORE.set(SETTINGS_KEY_SLEEP_MODE, false);
    if (await this.notifications.notificationTypeEnabled('SLEEP_MODE_DISABLED')) {
      await this.notifications.send(
        this.translate.instant('notifications.sleepModeDisabled.content')
      );
    }
  }

}
