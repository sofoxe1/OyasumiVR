import { Component, HostBinding, OnInit } from '@angular/core';
import { BaseModalComponent } from 'src-ui/app/components/base-modal/base-modal.component';
import { fade, fadeUp, triggerChildren, vshrink } from '../../../../../../utils/animations';
import { TranslateService } from '@ngx-translate/core';
import { getStringForDuration, getStringForDurationAwake, getStringForDurationSleep } from '../../tabs/sleep-detection-tab.component';
import { warn } from '@tauri-apps/plugin-log';

export interface DurationDisableSleepModeModalInputModel {
  duration: string | null;
  awake: string | null;
  sleep: string;
}

export interface DurationDisableSleepModeModalOutputModel {
  duration: string | null;
  awake: string | null;
  sleep: string;
}

@Component({
  selector: 'app-duration-disable-sleepmode-modal',
  templateUrl: './duration-disable-sleep-mode-modal.component.html',
  styleUrls: ['./duration-disable-mode-modal.component.scss'],
  animations: [fadeUp(), fade(), triggerChildren(), vshrink()],
  standalone: false,
})
export class DurationDisableSleepModeModalComponent
  extends BaseModalComponent<
    DurationDisableSleepModeModalInputModel,
    DurationDisableSleepModeModalOutputModel
  >
  implements OnInit, DurationDisableSleepModeModalInputModel
{
  duration: string | null = null;
  awake: string | null = null;
  sleep: string ="00:15";

  @HostBinding('[@fadeUp]') get fadeUp() {
    return;
  }

  constructor(private translate: TranslateService) {
    super();
  }

  ngOnInit(): void {
    if (this.duration && this.duration.length == 4) {
      this.duration = '0' + this.duration;
    }
    if (this.awake && this.awake.length == 4) {
      this.awake = '0' + this.awake;
    }
    if (this.sleep && this.sleep.length == 4) {
      this.sleep = '0' + this.sleep;
    }
    if (!this.duration || !this.duration.match(/[0-2][0-9]:[0-5][0-9]/g)) {
      warn('mallformed duration:' + this.duration);
      this.duration = '00:00';
    }
    if (!this.sleep || !this.sleep.match(/[0-2][0-9]:[0-5][0-9]/g)) {
      warn('mallformed sleep time:' + this.sleep);
      this.sleep = '00:15';
    }
    if (!this.awake || !this.awake.match(/[0-2][0-9]:[0-5][0-9]/g)) {
      warn('mallformed awake time:' + this.awake);
      this.awake = '00:00';
    }
  }

  save() {
    this.result = this;
    this.close();
  }

  protected getStringForDuration(duration: string | null) {
    if (!duration) {
      return '';
    }
    return getStringForDuration(this.translate, duration);
  }
  protected getStringForDurationAwake(awake: string | null) {
    if (!awake) {
      return '';
    }
    return getStringForDurationAwake(this.translate, awake);
  }
  protected getStringForDurationSleep(sleep: string | null) {
    if (!sleep) {
      return '';
    }
    return getStringForDurationSleep(this.translate, sleep);
  }
}
